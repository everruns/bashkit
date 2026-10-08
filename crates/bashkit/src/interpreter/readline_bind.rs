//! The `bind` builtin, as a non-interactive bash shows it: bashkit has no
//! readline, so bindings are recorded and reported, never acted on.
//!
//! Decisions:
//! - Defaults are readline's emacs keymap (`readline_defaults.rs`); changes
//!   (`bind 'seq: fn'`, `-x`, `-r`, `-u`, `set var val`) are kept per
//!   interpreter in [`Bindings`] (inside `CompletionState`), built on first
//!   use. Other keymaps (`-m vi`) start empty.
//! - THREAT[TM-DOS-132]: at most [`MAX_BINDINGS`] changes of at most
//!   [`MAX_BINDING_BYTES`] each; `bind -f` reads at most `max_input_bytes`
//!   from the VFS.
//! - Outside `bash -i` every call warns `line editing not enabled` on stderr,
//!   as bash does.

use super::{ExecResult, Interpreter};
use crate::error::Result;
use crate::parser::Redirect;

pub(crate) use super::readline_defaults::FUNCTION_NAMES;
use super::readline_defaults::{DEFAULT_EMACS_BINDINGS, VARIABLES};

/// THREAT[TM-DOS-132]: cap on recorded binding changes per interpreter.
pub(crate) const MAX_BINDINGS: usize = 1024;
/// THREAT[TM-DOS-132]: cap on one binding's text.
pub(crate) const MAX_BINDING_BYTES: usize = 4096;

const USAGE: &str = "bind: usage: bind [-lpsvPSVX] [-m keymap] [-f filename] [-q name] [-u name] [-r keyseq] [-x keyseq:shell-command] [keyseq:readline-function or readline-command]\n";

const KEYMAPS: &[&str] = &[
    "emacs",
    "emacs-standard",
    "emacs-meta",
    "emacs-ctlx",
    "vi",
    "vi-move",
    "vi-command",
    "vi-insert",
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Function(String),
    Macro(String),
    Command(String),
}

#[derive(Debug, Clone)]
struct Binding {
    keymap: &'static str,
    /// Key sequence as written (`\C-o\C-s`).
    seq: String,
    /// `None`: an unbound key (`bind -r`, `bind -u`).
    target: Option<Target>,
}

/// Binding changes and variable settings of one interpreter.
#[derive(Debug, Clone, Default)]
pub(crate) struct Bindings {
    changes: Vec<Binding>,
    variables: Vec<(String, String)>,
}

/// Canonical keymap for `-m NAME` (`emacs` is `emacs-standard`, `vi` is
/// `vi-command`).
fn keymap_name(name: &str) -> Option<&'static str> {
    match name {
        "emacs" | "emacs-standard" => Some("emacs"),
        "vi" | "vi-move" | "vi-command" => Some("vi"),
        other => KEYMAPS.iter().find(|k| **k == other).copied(),
    }
}

/// Readline key-sequence notation to bytes, for comparing and ordering.
fn keyseq_bytes(seq: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let chars: Vec<char> = seq.chars().collect();
    let mut i = 0;
    let mut meta = false;
    let mut control = false;
    while i < chars.len() {
        let mut byte: Option<u8> = None;
        if chars[i] == '\\' && i + 1 < chars.len() {
            let n = chars[i + 1];
            if (n == 'C' || n == 'M') && chars.get(i + 2) == Some(&'-') {
                if n == 'C' {
                    control = true;
                } else {
                    meta = true;
                }
                i += 3;
                continue;
            }
            i += 2;
            byte = Some(match n {
                'e' => 0x1b,
                'a' => 0x07,
                'b' => 0x08,
                'd' => 0x7f,
                'f' => 0x0c,
                'n' => b'\n',
                'r' => b'\r',
                't' => b'\t',
                'v' => 0x0b,
                c => c as u8,
            });
        }
        let mut b = byte.unwrap_or_else(|| {
            let c = chars[i];
            i += 1;
            c as u8
        });
        if control {
            b = if b == b'?' { 0x7f } else { b & 0x1f };
            control = false;
        }
        if meta {
            out.push(0x1b);
            meta = false;
        }
        out.push(b);
    }
    out
}

/// Strip the quotes of `"seq"` (or take a bare sequence) and split
/// `"seq": rest`.
fn split_binding(spec: &str) -> Option<(String, String)> {
    let spec = spec.trim_start();
    if let Some(body) = spec.strip_prefix('"') {
        let mut seq = String::new();
        let mut chars = body.char_indices();
        while let Some((idx, c)) = chars.next() {
            match c {
                '\\' => {
                    seq.push(c);
                    if let Some((_, n)) = chars.next() {
                        seq.push(n);
                    }
                }
                '"' => {
                    let rest = body[idx + 1..].trim_start();
                    let rest = rest.strip_prefix(':')?;
                    return Some((seq, rest.trim().to_string()));
                }
                c => seq.push(c),
            }
        }
        None
    } else {
        let (seq, rest) = spec.split_once(':')?;
        Some((seq.trim().to_string(), rest.trim().to_string()))
    }
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    for q in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
}

impl Bindings {
    /// Every live binding of `keymap`: defaults minus unbound keys, plus
    /// changes (a later change of the same keys wins).
    fn effective(&self, keymap: &str) -> Vec<(Vec<u8>, String, Target)> {
        let mut map: Vec<(Vec<u8>, String, Target)> = Vec::new();
        if keymap == "emacs" {
            for line in DEFAULT_EMACS_BINDINGS.lines() {
                if let Some((seq, func)) = split_binding(line) {
                    map.push((keyseq_bytes(&seq), seq, Target::Function(func)));
                }
            }
        }
        for change in self.changes.iter().filter(|c| c.keymap == keymap) {
            let bytes = keyseq_bytes(&change.seq);
            map.retain(|(b, ..)| *b != bytes);
            if let Some(target) = &change.target {
                map.push((bytes, change.seq.clone(), target.clone()));
            }
        }
        map
    }

    fn keys_for(&self, keymap: &str, function: &str) -> Vec<String> {
        let mut keys: Vec<(Vec<u8>, String)> = self
            .effective(keymap)
            .into_iter()
            .filter(|(_, _, t)| *t == Target::Function(function.to_string()))
            .map(|(b, s, _)| (b, s))
            .collect();
        keys.sort();
        keys.into_iter().map(|(_, s)| s).collect()
    }

    fn push(&mut self, binding: Binding) -> std::result::Result<(), String> {
        if binding.seq.len()
            + match &binding.target {
                Some(Target::Function(s) | Target::Macro(s) | Target::Command(s)) => s.len(),
                None => 0,
            }
            > MAX_BINDING_BYTES
        {
            return Err(format!(
                "bind: binding too long (limit {MAX_BINDING_BYTES} bytes)\n"
            ));
        }
        let bytes = keyseq_bytes(&binding.seq);
        self.changes
            .retain(|c| !(c.keymap == binding.keymap && keyseq_bytes(&c.seq) == bytes));
        if self.changes.len() >= MAX_BINDINGS {
            return Err(format!("bind: too many bindings (limit {MAX_BINDINGS})\n"));
        }
        self.changes.push(binding);
        Ok(())
    }

    fn variable(&self, name: &str) -> Option<String> {
        self.variables
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
            .or_else(|| {
                VARIABLES
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, v)| v.to_string())
            })
    }
}

impl Interpreter {
    fn bindings(&mut self) -> &mut Bindings {
        &mut self
            .completion
            .get_or_insert_with(Default::default)
            .bindings
    }

    /// The `bind` builtin.
    pub(super) async fn execute_bind_builtin(
        &mut self,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        let mut result = self.bind_builtin(args).await;
        if !self.interactive {
            let warn = self.diag("bind: warning: line editing not enabled\n");
            let mut err = crate::StreamData::from(warn);
            err.append(&result.stderr);
            result.stderr = err;
        }
        self.redirect_result(result, redirects).await
    }

    async fn bind_builtin(&mut self, args: &[String]) -> ExecResult {
        let mut keymap: &'static str = "emacs";
        let mut list_functions = false;
        let mut print = false;
        let mut print_readable = false;
        let mut macros = false;
        let mut macros_readable = false;
        let mut vars = false;
        let mut vars_readable = false;
        let mut commands = false;
        let mut queries = Vec::new();
        let mut unbind_functions = Vec::new();
        let mut removes = Vec::new();
        let mut shell_bindings = Vec::new();
        let mut files = Vec::new();
        let mut idx = 0;
        while idx < args.len() {
            let arg = &args[idx];
            if arg == "--" {
                idx += 1;
                break;
            }
            if !arg.starts_with('-') || arg.len() < 2 {
                break;
            }
            let chars: Vec<char> = arg[1..].chars().collect();
            let mut ci = 0;
            while ci < chars.len() {
                let c = chars[ci];
                ci += 1;
                match c {
                    'l' => list_functions = true,
                    'p' => print_readable = true,
                    'P' => print = true,
                    's' => macros_readable = true,
                    'S' => macros = true,
                    'v' => vars_readable = true,
                    'V' => vars = true,
                    'X' => commands = true,
                    'm' | 'f' | 'q' | 'u' | 'r' | 'x' => {
                        let value = if ci < chars.len() {
                            let v: String = chars[ci..].iter().collect();
                            ci = chars.len();
                            v
                        } else {
                            idx += 1;
                            match args.get(idx) {
                                Some(v) => v.clone(),
                                None => {
                                    return ExecResult::err(
                                        format!(
                                            "{}{USAGE}",
                                            self.diag(format!(
                                                "bind: -{c}: option requires an argument\n"
                                            ))
                                        ),
                                        2,
                                    );
                                }
                            }
                        };
                        match c {
                            'm' => match keymap_name(&value) {
                                Some(k) => keymap = k,
                                None => {
                                    return ExecResult::err(
                                        self.diag(format!(
                                            "bind: `{value}': invalid keymap name\n"
                                        )),
                                        1,
                                    );
                                }
                            },
                            'f' => files.push(value),
                            'q' => queries.push(value),
                            'u' => unbind_functions.push(value),
                            'r' => removes.push(value),
                            _ => shell_bindings.push(value),
                        }
                    }
                    other => {
                        return ExecResult::err(
                            format!(
                                "{}{USAGE}",
                                self.diag(format!("bind: -{other}: invalid option\n"))
                            ),
                            2,
                        );
                    }
                }
            }
            idx += 1;
        }
        let rest: Vec<String> = args[idx..].to_vec();
        let mut out = String::new();
        let mut err = String::new();
        let mut status = 0;

        for file in &files {
            let path = self.resolve_path(file);
            match self.fs.read_file(&path).await {
                Ok(bytes) if bytes.len() <= self.limits.max_input_bytes => {
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    for line in text.lines() {
                        let line = line.trim();
                        if line.is_empty() || line.starts_with('#') || line.starts_with('$') {
                            continue;
                        }
                        if let Err(e) = self.bind_line(keymap, line) {
                            err.push_str(&self.diag(e));
                            status = 1;
                        }
                    }
                }
                _ => {
                    err.push_str(&self.diag(format!(
                        "bind: {file}: cannot read: No such file or directory\n"
                    )));
                    status = 1;
                }
            }
        }

        if list_functions {
            for name in FUNCTION_NAMES {
                out.push_str(name);
                out.push('\n');
            }
        }
        if print || print_readable {
            out.push('\n');
            let effective = self.bindings().effective(keymap);
            for name in FUNCTION_NAMES {
                let mut keys: Vec<(Vec<u8>, String)> = effective
                    .iter()
                    .filter(|(_, _, t)| *t == Target::Function(name.to_string()))
                    .map(|(b, s, _)| (b.clone(), s.clone()))
                    .collect();
                keys.sort();
                if print_readable {
                    if keys.is_empty() {
                        out.push_str(&format!("# {name} (not bound)\n"));
                    }
                    for (_, seq) in &keys {
                        out.push_str(&format!("\"{seq}\": {name}\n"));
                    }
                } else if keys.is_empty() {
                    out.push_str(&format!("{name} is not bound to any keys\n"));
                } else {
                    let list: Vec<String> = keys.iter().map(|(_, s)| format!("\"{s}\"")).collect();
                    out.push_str(&format!("{name} can be found on {}.\n", list.join(", ")));
                }
            }
        }
        if macros || macros_readable || commands {
            let effective = self.bindings().effective(keymap);
            let mut items: Vec<_> = effective.into_iter().collect();
            items.sort_by(|a, b| a.0.cmp(&b.0));
            for (_, seq, target) in items {
                match target {
                    Target::Macro(m) if macros_readable => {
                        out.push_str(&format!("\"{seq}\": \"{m}\"\n"))
                    }
                    Target::Macro(m) if macros => out.push_str(&format!("{seq} outputs {m}\n")),
                    Target::Command(c) if commands => {
                        out.push_str(&format!("\"{seq}\": \"{c}\"\n"))
                    }
                    _ => {}
                }
            }
        }
        if vars || vars_readable {
            let bindings = self.bindings().clone();
            for (name, _) in VARIABLES {
                let value = bindings.variable(name).unwrap_or_default();
                if vars_readable {
                    out.push_str(&format!("set {name} {value}\n"));
                } else {
                    out.push_str(&format!("{name} is set to `{value}'\n"));
                }
            }
        }
        for name in &queries {
            if !FUNCTION_NAMES.contains(&name.as_str()) {
                err.push_str(&self.diag(format!("bind: `{name}': unknown function name\n")));
                status = 1;
                continue;
            }
            let keys = self.bindings().keys_for(keymap, name);
            if keys.is_empty() {
                out.push_str(&format!("{name} is not bound to any keys.\n"));
                status = 1;
            } else {
                let list: Vec<String> = keys.iter().map(|s| format!("\"{s}\"")).collect();
                out.push_str(&format!("{name} can be invoked via {}.\n", list.join(", ")));
            }
        }
        for name in &unbind_functions {
            if !FUNCTION_NAMES.contains(&name.as_str()) {
                err.push_str(&self.diag(format!("bind: `{name}': unknown function name\n")));
                status = 1;
                continue;
            }
            for seq in self.bindings().keys_for(keymap, name) {
                if let Err(e) = self.bindings().push(Binding {
                    keymap,
                    seq,
                    target: None,
                }) {
                    err.push_str(&self.diag(e));
                    status = 1;
                }
            }
        }
        for seq in &removes {
            if let Err(e) = self.bindings().push(Binding {
                keymap,
                seq: seq.clone(),
                target: None,
            }) {
                err.push_str(&self.diag(e));
                status = 1;
            }
        }
        for spec in &shell_bindings {
            let Some((seq, command)) = split_binding(spec) else {
                err.push_str(&self.diag("bind: no colon in key sequence binding\n"));
                status = 1;
                continue;
            };
            if let Err(e) = self.bindings().push(Binding {
                keymap,
                seq,
                target: Some(Target::Command(unquote(&command))),
            }) {
                err.push_str(&self.diag(e));
                status = 1;
            }
        }
        for line in &rest {
            if let Err(e) = self.bind_line(keymap, line) {
                err.push_str(&self.diag(e));
                status = 1;
            }
        }
        let mut result = ExecResult::with_code(out, status);
        result.stderr = err.into();
        result
    }

    /// One inputrc line: `"seq": function`, `"seq": "macro"`, `set var val`.
    fn bind_line(&mut self, keymap: &'static str, line: &str) -> std::result::Result<(), String> {
        if let Some(rest) = line.trim_start().strip_prefix("set ") {
            let mut parts = rest.split_whitespace();
            let name = parts.next().unwrap_or("").to_string();
            let value = parts.collect::<Vec<_>>().join(" ");
            if VARIABLES.iter().any(|(n, _)| *n == name) {
                let bindings = self.bindings();
                if bindings.variables.len() >= MAX_BINDINGS {
                    return Err(format!("bind: too many bindings (limit {MAX_BINDINGS})\n"));
                }
                bindings.variables.retain(|(n, _)| *n != name);
                bindings.variables.push((name, value));
            }
            return Ok(());
        }
        let Some((seq, rhs)) = split_binding(line) else {
            return Ok(());
        };
        let target = if rhs.starts_with('"') || rhs.starts_with('\'') {
            Target::Macro(unquote(&rhs))
        } else {
            Target::Function(rhs)
        };
        self.bindings().push(Binding {
            keymap,
            seq,
            target: Some(target),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyseqs_order_like_readline() {
        assert!(keyseq_bytes("\\C-g") < keyseq_bytes("\\C-x\\C-g"));
        assert!(keyseq_bytes("\\C-x\\C-g") < keyseq_bytes("\\M-\\C-g"));
        assert_eq!(keyseq_bytes("\\C-o\\C-s"), vec![0x0f, 0x13]);
    }

    #[test]
    fn default_yank_is_c_y() {
        let b = Bindings::default();
        assert_eq!(b.keys_for("emacs", "yank"), vec!["\\C-y".to_string()]);
        assert!(b.keys_for("emacs", "vi-subst").is_empty());
    }

    #[test]
    fn binding_changes_are_capped() {
        let mut b = Bindings::default();
        for i in 0..MAX_BINDINGS {
            b.push(Binding {
                keymap: "emacs",
                seq: format!("\\C-x{i}"),
                target: Some(Target::Command("x".into())),
            })
            .unwrap();
        }
        assert!(
            b.push(Binding {
                keymap: "emacs",
                seq: "\\C-zz".into(),
                target: None,
            })
            .is_err()
        );
    }
}
