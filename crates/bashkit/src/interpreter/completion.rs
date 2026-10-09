//! Programmable completion builtins: `complete`, `compgen`, `compopt`.
//!
//! Decisions:
//! - Interpreter-level (not registered builtins): `compgen -W` expands its
//!   word list in the current shell, `-F` calls a shell function and reads
//!   `COMPREPLY` back, `-X` matches with the shell's (ext)glob matcher.
//! - Completion specs live in [`CompletionState`], one per interpreter,
//!   built on first use and cloned into subshells like other shell state.
//!   Never global: tenants sharing a process never see each other's specs.
//! - THREAT[TM-DOS-132]: at most [`MAX_COMPLETION_SPECS`] specs, each at most
//!   [`MAX_COMPLETION_SPEC_BYTES`] of option text; `complete` fails past
//!   either cap instead of growing.
//! - Generation order follows bash's `gen_compspec_completions`: actions
//!   (directories and files last), `-G`, `-W`, `-F`, `-C`; then `-X`, `-P`/`-S`,
//!   `-o plusdirs`, and `-o dirnames`/`-o default` when nothing matched.
//!   Each action's names are sorted (bash's order there is hash order).
//! - Users, groups, hosts and services come from the sandbox (`/etc/passwd`
//!   etc. on the VFS when present, else the configured user and hostname),
//!   never the host.

use super::{ExecResult, Interpreter, VarAttrs};
use crate::error::Result;
use crate::parser::Redirect;

/// THREAT[TM-DOS-132]: cap on stored completion specs per interpreter.
pub(crate) const MAX_COMPLETION_SPECS: usize = 1024;
/// THREAT[TM-DOS-132]: cap on one spec's option text (`-W` list etc.).
pub(crate) const MAX_COMPLETION_SPEC_BYTES: usize = 64 * 1024;

/// Completion actions in bash's `complete -p` order: the letter forms
/// first, then the `-A`-only names.
const ACTIONS: &[(&str, Option<char>)] = &[
    ("alias", Some('a')),
    ("builtin", Some('b')),
    ("command", Some('c')),
    ("directory", Some('d')),
    ("export", Some('e')),
    ("file", Some('f')),
    ("group", Some('g')),
    ("job", Some('j')),
    ("keyword", Some('k')),
    ("service", Some('s')),
    ("user", Some('u')),
    ("variable", Some('v')),
    ("arrayvar", None),
    ("binding", None),
    ("disabled", None),
    ("enabled", None),
    ("function", None),
    ("helptopic", None),
    ("hostname", None),
    ("running", None),
    ("setopt", None),
    ("shopt", None),
    ("signal", None),
    ("stopped", None),
];

/// The order actions generate in (bash: directories and files last).
const GENERATION_ORDER: &[&str] = &[
    "alias",
    "arrayvar",
    "binding",
    "builtin",
    "command",
    "disabled",
    "enabled",
    "export",
    "function",
    "group",
    "helptopic",
    "hostname",
    "job",
    "keyword",
    "running",
    "service",
    "setopt",
    "shopt",
    "signal",
    "stopped",
    "user",
    "variable",
    "directory",
    "file",
];

/// `-o` options, in `complete -p` order.
const COMP_OPTIONS: &[&str] = &[
    "bashdefault",
    "default",
    "dirnames",
    "filenames",
    "noquote",
    "nosort",
    "nospace",
    "plusdirs",
];

/// Reserved words, in `compgen -k` order.
const KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "case", "esac", "for", "select", "while", "until", "do",
    "done", "in", "function", "time", "{", "}", "!", "[[", "]]", "coproc",
];

const SIGNALS: &[&str] = &[
    "SIGHUP",
    "SIGINT",
    "SIGQUIT",
    "SIGILL",
    "SIGTRAP",
    "SIGABRT",
    "SIGBUS",
    "SIGFPE",
    "SIGKILL",
    "SIGUSR1",
    "SIGSEGV",
    "SIGUSR2",
    "SIGPIPE",
    "SIGALRM",
    "SIGTERM",
    "SIGSTKFLT",
    "SIGCHLD",
    "SIGCONT",
    "SIGSTOP",
    "SIGTSTP",
    "SIGTTIN",
    "SIGTTOU",
    "SIGURG",
    "SIGXCPU",
    "SIGXFSZ",
    "SIGVTALRM",
    "SIGPROF",
    "SIGWINCH",
    "SIGIO",
    "SIGPWR",
    "SIGSYS",
];

const COMPLETE_USAGE: &str = "complete: usage: complete [-abcdefgjksuv] [-pr] [-DEI] [-o option] [-A action] [-G globpat] [-W wordlist] [-F function] [-C command] [-X filterpat] [-P prefix] [-S suffix] [name ...]\n";
const COMPGEN_USAGE: &str = "compgen: usage: compgen [-abcdefgjksuv] [-o option] [-A action] [-G globpat] [-W wordlist] [-F function] [-C command] [-X filterpat] [-P prefix] [-S suffix] [word]\n";
const COMPOPT_USAGE: &str = "compopt: usage: compopt [-o|+o option] [-DEI] [name ...]\n";

/// Spec names bash uses for `complete -D`, `-E` and `-I`.
const DEFAULT_SPEC: &str = "\0default";
const EMPTY_SPEC: &str = "\0empty";
const INITIAL_SPEC: &str = "\0initial";

/// One completion specification (`complete ... name`).
#[derive(Debug, Clone, Default)]
pub(crate) struct CompSpec {
    /// Bit `i` set: `ACTIONS[i]` is on.
    actions: u32,
    /// Bit `i` set: `COMP_OPTIONS[i]` is on.
    options: u8,
    glob: Option<String>,
    words: Option<String>,
    prefix: Option<String>,
    suffix: Option<String>,
    filter: Option<String>,
    command: Option<String>,
    function: Option<String>,
}

impl CompSpec {
    fn text_bytes(&self) -> usize {
        [
            &self.glob,
            &self.words,
            &self.prefix,
            &self.suffix,
            &self.filter,
            &self.command,
            &self.function,
        ]
        .iter()
        .map(|s| s.as_ref().map_or(0, String::len))
        .sum()
    }

    fn has_action(&self, name: &str) -> bool {
        ACTIONS
            .iter()
            .position(|(n, _)| *n == name)
            .is_some_and(|i| self.actions & (1 << i) != 0)
    }

    fn has_option(&self, name: &str) -> bool {
        COMP_OPTIONS
            .iter()
            .position(|n| *n == name)
            .is_some_and(|i| self.options & (1 << i) != 0)
    }

    /// `complete -p` form.
    fn print(&self, name: &str) -> String {
        let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
        let mut out = String::from("complete ");
        for (i, opt) in COMP_OPTIONS.iter().enumerate() {
            if self.options & (1 << i) != 0 {
                out.push_str(&format!("-o {opt} "));
            }
        }
        for (i, (action, letter)) in ACTIONS.iter().enumerate() {
            if self.actions & (1 << i) != 0 {
                match letter {
                    Some(c) => out.push_str(&format!("-{c} ")),
                    None => out.push_str(&format!("-A {action} ")),
                }
            }
        }
        for (flag, value) in [
            ("-G", &self.glob),
            ("-W", &self.words),
            ("-P", &self.prefix),
            ("-S", &self.suffix),
            ("-X", &self.filter),
            ("-C", &self.command),
        ] {
            if let Some(v) = value {
                out.push_str(&format!("{flag} {} ", quote(v)));
            }
        }
        if let Some(f) = &self.function {
            out.push_str(&format!("-F {f} "));
        }
        out.push_str(match name {
            DEFAULT_SPEC => "-D",
            EMPTY_SPEC => "-E",
            INITIAL_SPEC => "-I",
            other => other,
        });
        out.push('\n');
        out
    }
}

/// Per-interpreter completion and readline-binding state.
#[derive(Debug, Clone, Default)]
pub(crate) struct CompletionState {
    /// Specs in definition order (bash prints them in hash order).
    specs: Vec<(String, CompSpec)>,
    /// A spec was ever defined (before that, `complete -r name` is quiet).
    specs_created: bool,
    /// Completion functions running now (`compopt` needs one).
    active_functions: usize,
    /// Bytes of `compgen`'s stdout/stderr a completion function already
    /// streamed (scratch for one `compgen` call).
    streamed: (usize, usize),
    /// `bind` key bindings and variables (see `readline_bind.rs`).
    pub(crate) bindings: super::readline_bind::Bindings,
}

/// Parsed `complete`/`compgen` options.
#[derive(Default)]
struct CompArgs {
    spec: CompSpec,
    print: bool,
    remove: bool,
    names_default: bool,
    names_empty: bool,
    names_initial: bool,
    rest: Vec<String>,
}

enum ParseError {
    Message(String, i32),
}

/// Parse bash-style options (`-abc`, `-W words`, `-Wwords`). `builtin` is
/// `complete` or `compgen`; only `complete` takes `-p -r -D -E -I`.
fn parse_comp_args(builtin: &str, args: &[String]) -> std::result::Result<CompArgs, ParseError> {
    let mut parsed = CompArgs::default();
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
            if let Some(i) = ACTIONS.iter().position(|(_, l)| *l == Some(c)) {
                parsed.spec.actions |= 1 << i;
                continue;
            }
            match c {
                'p' | 'r' | 'D' | 'E' | 'I' if builtin == "complete" => match c {
                    'p' => parsed.print = true,
                    'r' => parsed.remove = true,
                    'D' => parsed.names_default = true,
                    'E' => parsed.names_empty = true,
                    _ => parsed.names_initial = true,
                },
                'A' | 'o' | 'G' | 'W' | 'F' | 'C' | 'X' | 'P' | 'S' => {
                    let value = if ci < chars.len() {
                        let v: String = chars[ci..].iter().collect();
                        ci = chars.len();
                        v
                    } else {
                        idx += 1;
                        match args.get(idx) {
                            Some(v) => v.clone(),
                            None => {
                                return Err(ParseError::Message(
                                    format!("{builtin}: -{c}: option requires an argument\n"),
                                    2,
                                ));
                            }
                        }
                    };
                    match c {
                        'A' => match ACTIONS.iter().position(|(n, _)| *n == value) {
                            Some(i) => parsed.spec.actions |= 1 << i,
                            None => {
                                return Err(ParseError::Message(
                                    format!("{builtin}: {value}: invalid action name\n"),
                                    2,
                                ));
                            }
                        },
                        'o' => match COMP_OPTIONS.iter().position(|n| *n == value) {
                            Some(i) => parsed.spec.options |= 1 << i,
                            None => {
                                return Err(ParseError::Message(
                                    format!("{builtin}: {value}: invalid option name\n"),
                                    2,
                                ));
                            }
                        },
                        'G' => parsed.spec.glob = Some(value),
                        'W' => parsed.spec.words = Some(value),
                        'F' => parsed.spec.function = Some(value),
                        'C' => parsed.spec.command = Some(value),
                        'X' => parsed.spec.filter = Some(value),
                        'P' => parsed.spec.prefix = Some(value),
                        _ => parsed.spec.suffix = Some(value),
                    }
                }
                other => {
                    return Err(ParseError::Message(
                        format!("{builtin}: -{other}: invalid option\n"),
                        2,
                    ));
                }
            }
        }
        idx += 1;
    }
    parsed.rest = args[idx..].to_vec();
    Ok(parsed)
}

/// Single-quote a word for shell text we run ourselves.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Split a `-W` word list at unquoted, unescaped IFS characters (bash's
/// `split_at_delims`); quoting and `$(...)` stay inside a word.
fn split_wordlist(text: &str, ifs: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut chars = text.chars().peekable();
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            cur.push(c);
            if c == '\\' && q == '"' {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\\' => {
                cur.push(c);
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            '\'' | '"' => {
                quote = Some(c);
                cur.push(c);
            }
            '$' if matches!(chars.peek(), Some('(' | '{')) => {
                cur.push(c);
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
                depth += 1;
            }
            ')' | '}' if depth > 0 => {
                depth -= 1;
                cur.push(c);
            }
            '(' | '{' if depth > 0 => {
                depth += 1;
                cur.push(c);
            }
            c if depth == 0 && ifs.contains(c) => {
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

impl Interpreter {
    fn completion_state(&mut self) -> &mut CompletionState {
        self.completion.get_or_insert_with(Default::default)
    }

    /// Run shell text in the current shell (completion functions, `-C`).
    async fn run_completion_text(&mut self, text: &str) -> Result<ExecResult> {
        let script = self.parse_embedded_script(text).await?;
        self.execute_command_sequence(&script.commands).await
    }

    /// The `complete` builtin.
    pub(super) async fn execute_complete_builtin(
        &mut self,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        let result = self.complete_builtin(args);
        self.redirect_result(result, redirects).await
    }

    fn complete_builtin(&mut self, args: &[String]) -> ExecResult {
        let parsed = match parse_comp_args("complete", args) {
            Ok(p) => p,
            Err(ParseError::Message(msg, code)) => {
                return ExecResult::err(format!("{}{COMPLETE_USAGE}", self.diag(msg)), code);
            }
        };
        let mut names: Vec<String> = parsed.rest.clone();
        for (on, name) in [
            (parsed.names_default, DEFAULT_SPEC),
            (parsed.names_empty, EMPTY_SPEC),
            (parsed.names_initial, INITIAL_SPEC),
        ] {
            if on {
                names.push(name.to_string());
            }
        }
        let has_spec =
            parsed.spec.actions != 0 || parsed.spec.options != 0 || parsed.spec.text_bytes() > 0;

        if parsed.print || (args.is_empty()) {
            return self.complete_print(&names);
        }
        if parsed.remove {
            let state = self.completion_state();
            if names.is_empty() || !state.specs_created {
                state.specs.clear();
                return ExecResult::ok(String::new());
            }
            let mut err = String::new();
            for name in &names {
                match state.specs.iter().position(|(n, _)| n == name) {
                    Some(i) => {
                        state.specs.remove(i);
                    }
                    None => {
                        err.push_str(&format!("complete: {name}: no completion specification\n"))
                    }
                }
            }
            return if err.is_empty() {
                ExecResult::ok(String::new())
            } else {
                ExecResult::err(self.diag(err), 1)
            };
        }
        if names.is_empty() {
            if has_spec {
                return ExecResult::err(COMPLETE_USAGE, 2);
            }
            return self.complete_print(&names);
        }
        if parsed.spec.text_bytes() > MAX_COMPLETION_SPEC_BYTES {
            return ExecResult::err(
                self.diag(format!(
                    "complete: completion specification too large (limit {MAX_COMPLETION_SPEC_BYTES} bytes)\n"
                )),
                1,
            );
        }
        let state = self.completion_state();
        for name in names {
            if let Some(slot) = state.specs.iter_mut().find(|(n, _)| *n == name) {
                slot.1 = parsed.spec.clone();
            } else if state.specs.len() >= MAX_COMPLETION_SPECS {
                return ExecResult::err(
                    format!(
                        "complete: too many completion specifications (limit {MAX_COMPLETION_SPECS})\n"
                    ),
                    1,
                );
            } else {
                state.specs.push((name, parsed.spec.clone()));
                state.specs_created = true;
            }
        }
        ExecResult::ok(String::new())
    }

    fn complete_print(&mut self, names: &[String]) -> ExecResult {
        let state = self.completion_state();
        let mut out = String::new();
        if names.is_empty() {
            for (name, spec) in &state.specs {
                out.push_str(&spec.print(name));
            }
            return ExecResult::ok(out);
        }
        let mut err = String::new();
        for name in names {
            match state.specs.iter().find(|(n, _)| n == name) {
                Some((n, spec)) => out.push_str(&spec.print(n)),
                None => err.push_str(&format!("complete: {name}: no completion specification\n")),
            }
        }
        if err.is_empty() {
            ExecResult::ok(out)
        } else {
            let mut r = ExecResult::err(self.diag(err), 1);
            r.stdout = out.into();
            r
        }
    }

    /// The `compopt` builtin: changes options of a spec, or of the running
    /// completion (outside one it fails, as bash does).
    pub(super) async fn execute_compopt_builtin(
        &mut self,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        let result = self.compopt_builtin(args);
        self.redirect_result(result, redirects).await
    }

    fn compopt_builtin(&mut self, args: &[String]) -> ExecResult {
        let mut set: u8 = 0;
        let mut unset: u8 = 0;
        let mut names = Vec::new();
        let mut idx = 0;
        while idx < args.len() {
            let arg = &args[idx];
            let on = arg.starts_with('-');
            if arg == "--" {
                idx += 1;
                break;
            }
            if !(on || arg.starts_with('+')) || arg.len() < 2 {
                break;
            }
            for c in arg[1..].chars() {
                match c {
                    'o' => {
                        idx += 1;
                        let Some(value) = args.get(idx) else {
                            return ExecResult::err(
                                format!(
                                    "{}{COMPOPT_USAGE}",
                                    self.diag("compopt: -o: option requires an argument\n")
                                ),
                                2,
                            );
                        };
                        let Some(i) = COMP_OPTIONS.iter().position(|n| n == value) else {
                            return ExecResult::err(
                                self.diag(format!("compopt: {value}: invalid option name\n")),
                                2,
                            );
                        };
                        if on {
                            set |= 1 << i;
                        } else {
                            unset |= 1 << i;
                        }
                    }
                    'D' => names.push(DEFAULT_SPEC.to_string()),
                    'E' => names.push(EMPTY_SPEC.to_string()),
                    'I' => names.push(INITIAL_SPEC.to_string()),
                    other => {
                        return ExecResult::err(
                            format!(
                                "{}{COMPOPT_USAGE}",
                                self.diag(format!("compopt: -{other}: invalid option\n"))
                            ),
                            2,
                        );
                    }
                }
            }
            idx += 1;
        }
        names.extend(args[idx..].iter().cloned());
        let state = self.completion_state();
        if names.is_empty() {
            if state.active_functions == 0 {
                return ExecResult::err(
                    self.diag("compopt: not currently executing completion function\n"),
                    1,
                );
            }
            return ExecResult::ok(String::new());
        }
        let mut err = String::new();
        for name in &names {
            match state.specs.iter_mut().find(|(n, _)| n == name) {
                Some((_, spec)) => spec.options = (spec.options | set) & !unset,
                None => err.push_str(&format!("compopt: {name}: no completion specification\n")),
            }
        }
        if err.is_empty() {
            ExecResult::ok(String::new())
        } else {
            ExecResult::err(self.diag(err), 1)
        }
    }

    /// The `compgen` builtin.
    pub(super) async fn execute_compgen_builtin(
        &mut self,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        let region = self.enter_output_region(redirects);
        let emit_before = self.output_emit_count;
        self.completion_state().streamed = (0, 0);
        let result = self.compgen_builtin(args).await;
        // A completion function streamed its own output (and the warning
        // before it): stream the rest after it.
        if let Ok(r) = &result
            && self.output_emit_count != emit_before
        {
            let (out_len, err_len) = self.completion_state().streamed;
            let out = r.stdout.suffix_from(out_len);
            let err = r.stderr.suffix_from(err_len);
            let count = self.output_emit_count;
            self.maybe_emit_output(&out, &err, count);
        }
        self.leave_output_region(region);
        let result = result?;
        self.redirect_result(result, redirects).await
    }

    async fn compgen_builtin(&mut self, args: &[String]) -> Result<ExecResult> {
        let parsed = match parse_comp_args("compgen", args) {
            Ok(p) => p,
            Err(ParseError::Message(msg, code)) => {
                return Ok(ExecResult::err(
                    format!("{}{COMPGEN_USAGE}", self.diag(msg)),
                    code,
                ));
            }
        };
        let spec = parsed.spec;
        let word = parsed.rest.first().cloned().unwrap_or_default();
        let mut stdout = crate::StreamData::new();
        let mut stderr = String::new();
        let mut matches: Vec<String> = Vec::new();

        for action in GENERATION_ORDER {
            if spec.has_action(action) {
                let mut names = self.action_matches(action, &word).await;
                matches.append(&mut names);
            }
        }

        if let Some(glob) = &spec.glob {
            matches.extend(self.glob_matches(glob).await);
        }

        if let Some(words) = &spec.words {
            match self.expand_wordlist(words).await {
                Ok(list) => matches.extend(list.into_iter().filter(|w| w.starts_with(&word))),
                Err(err) => {
                    let r = ExecResult::err(err, 1);
                    return Ok(r);
                }
            }
        }

        if let Some(func) = &spec.function {
            stderr.push_str(&self.diag("compgen: warning: -F option may not work as you expect\n"));
            let warn = crate::StreamData::from(std::mem::take(&mut stderr));
            let count = self.output_emit_count;
            self.maybe_emit_output(&crate::StreamData::new(), &warn, count);
            let mut warn_kept = warn;
            match self.completion_function_matches(func, &word).await? {
                Ok((out, err, list)) => {
                    stdout.append(&out);
                    warn_kept.append(&err);
                    matches.extend(list);
                }
                Err((out, err)) => {
                    stdout.append(&out);
                    warn_kept.append(&err);
                    self.completion_state().streamed = (stdout.len(), warn_kept.len());
                    let mut r = ExecResult::with_code(String::new(), 1);
                    r.stdout = stdout;
                    r.stderr = warn_kept;
                    return Ok(r);
                }
            }
            stderr = warn_kept.text_lossy().into_owned();
            self.completion_state().streamed = (stdout.len(), stderr.len());
        }

        if let Some(cmd) = &spec.command {
            let warn = self.diag("compgen: warning: -C option may not work as you expect\n");
            let text = format!("{cmd} compgen {} ''", shell_quote(&word));
            let saved_cb = self.output_callback.take();
            let r = self.run_completion_text(&text).await;
            self.output_callback = saved_cb;
            let r = r?;
            stderr.push_str(&warn);
            stderr.push_str(&r.stderr.text_lossy());
            matches.extend(r.stdout.text_lossy().lines().map(str::to_string));
        }

        if let Some(filter) = &spec.filter {
            let (negate, pattern) = match filter.strip_prefix('!') {
                Some(p) => (true, p),
                None => (false, filter.as_str()),
            };
            let pattern = substitute_ampersand(pattern, &word);
            matches.retain(|m| self.pattern_matches(m, &pattern) == negate);
        }

        if spec.prefix.is_some() || spec.suffix.is_some() {
            let pre = spec.prefix.clone().unwrap_or_default();
            let suf = spec.suffix.clone().unwrap_or_default();
            for m in &mut matches {
                *m = format!("{pre}{m}{suf}");
            }
        }

        if spec.has_option("plusdirs") {
            let mut dirs = self.path_matches(&word, true).await.unwrap_or_default();
            matches.append(&mut dirs);
        }
        if matches.is_empty() && spec.has_option("dirnames") {
            matches = self.path_matches(&word, true).await.unwrap_or_default();
        }
        if matches.is_empty() && (spec.has_option("default") || spec.has_option("bashdefault")) {
            matches = self.path_matches(&word, false).await.unwrap_or_default();
        }

        let generated_anything = spec.actions != 0
            || spec.options != 0
            || [
                &spec.glob,
                &spec.words,
                &spec.function,
                &spec.command,
                &spec.prefix,
                &spec.suffix,
                &spec.filter,
            ]
            .iter()
            .any(|v| v.is_some());
        let mut out = String::new();
        for m in &matches {
            out.push_str(m);
            out.push('\n');
        }
        stdout.append(&crate::StreamData::from(out));
        let code = i32::from(matches.is_empty() && generated_anything);
        let mut r = ExecResult::with_code(String::new(), code);
        r.stdout = stdout;
        r.stderr = stderr.into();
        Ok(r)
    }

    /// Call completion function `func` as `compgen -F` does: `$1` is
    /// `compgen`, `$2` the word, `$3` the previous word (empty), with the
    /// `COMP_*` variables bound for the call. Returns the function's own
    /// output and the `COMPREPLY` words, or `Err` when it failed fatally.
    #[allow(clippy::type_complexity)]
    async fn completion_function_matches(
        &mut self,
        func: &str,
        word: &str,
    ) -> Result<
        std::result::Result<
            (crate::StreamData, crate::StreamData, Vec<String>),
            (crate::StreamData, crate::StreamData),
        >,
    > {
        let text = format!(
            "COMP_WORDS=(); COMP_CWORD=-1; COMP_LINE=''; COMP_POINT=0; {} compgen {} ''",
            shell_quote(func),
            shell_quote(word)
        );
        self.completion_state().active_functions += 1;
        let result = self.run_completion_text(&text).await;
        self.completion_state().active_functions -= 1;
        let reply = self.read_compreply();
        let _ = self
            .run_completion_text("unset -v COMP_WORDS COMP_CWORD COMP_LINE COMP_POINT COMPREPLY")
            .await;
        let result = result?;
        if matches!(result.control_flow, super::ControlFlow::Abort) {
            return Ok(Err((result.stdout, result.stderr)));
        }
        Ok(Ok((result.stdout, result.stderr, reply)))
    }

    fn read_compreply(&self) -> Vec<String> {
        if let Some(arr) = self.scoped.arrays.get("COMPREPLY") {
            let mut keys: Vec<&usize> = arr.keys().collect();
            keys.sort_unstable();
            return keys.into_iter().map(|k| arr[k].clone()).collect();
        }
        match self.scoped.variables.get("COMPREPLY") {
            Some(v) => vec![v.clone()],
            None => Vec::new(),
        }
    }

    /// `-W`: split at IFS (quotes and escapes protect delimiters), then
    /// expand each word; an expansion error fails `compgen`.
    async fn expand_wordlist(&mut self, words: &str) -> std::result::Result<Vec<String>, String> {
        let ifs = if self.is_variable_set("IFS") {
            self.expand_variable("IFS")
        } else {
            " \t\n".to_string()
        };
        let mut out = Vec::new();
        for raw in split_wordlist(words, &ifs) {
            // A parse error (`${`) fails `compgen`; the word itself is one
            // word even when it holds blanks that are not in IFS.
            if has_unclosed_parameter(&raw) {
                return Err(self.diag(format!("{raw}: bad substitution\n")));
            }
            let script = match self
                .parse_embedded_script(&format!(": {}", escape_blanks(&raw)))
                .await
            {
                Ok(script) => script,
                Err(e) => return Err(self.diag(format!("{e}\n"))),
            };
            let word = match script.commands.as_slice() {
                [crate::parser::Command::Simple(cmd)] if cmd.args.len() == 1 => cmd.args[0].clone(),
                _ => crate::parser::Word::literal(raw.clone()),
            };
            let fields = match Box::pin(self.expand_word_to_fields(&word)).await {
                Ok(f) => f,
                Err(crate::error::Error::LineAbort(msg)) => return Err(msg),
                Err(e) => return Err(self.diag(format!("{e}\n"))),
            };
            if let Some(crate::error::Error::LineAbort(msg)) = self.pending_arith_abort() {
                return Err(msg);
            }
            out.extend(fields);
        }
        Ok(out)
    }

    /// `-G`: names in the current directory matching the glob.
    async fn glob_matches(&mut self, glob: &str) -> Vec<String> {
        let (dir_part, pattern) = match glob.rfind('/') {
            Some(i) => (&glob[..=i], &glob[i + 1..]),
            None => ("", glob),
        };
        let dir = if dir_part.is_empty() {
            self.cwd.clone()
        } else {
            self.resolve_path(dir_part)
        };
        let Ok(entries) = self.fs.read_dir(&dir).await else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .into_iter()
            .map(|e| e.name)
            .filter(|n| pattern.starts_with('.') || !n.starts_with('.'))
            .filter(|n| self.pattern_matches(n, pattern))
            .map(|n| format!("{dir_part}{n}"))
            .collect();
        names.sort();
        names
    }

    /// File (`dirs_only` false) or directory names completing `word`,
    /// keeping its directory part (`spec/t` gives `spec/testdata`).
    /// `None` when that directory cannot be read.
    async fn path_matches(&mut self, word: &str, dirs_only: bool) -> Option<Vec<String>> {
        let (dir_part, base) = match word.rfind('/') {
            Some(i) => (&word[..=i], &word[i + 1..]),
            None => ("", word),
        };
        let expanded_dir = match dir_part.strip_prefix("~/") {
            Some(rest) => format!("{}/{rest}", self.expand_variable("HOME")),
            None => dir_part.to_string(),
        };
        let dir = if expanded_dir.is_empty() {
            self.cwd.clone()
        } else {
            self.resolve_path(&expanded_dir)
        };
        let entries = self.fs.read_dir(&dir).await.ok()?;
        let mut names = Vec::new();
        for entry in entries {
            if !entry.name.starts_with(base)
                || (entry.name.starts_with('.') && !base.starts_with('.'))
            {
                continue;
            }
            if dirs_only {
                let is_dir = if entry.metadata.file_type.is_dir() {
                    true
                } else if entry.metadata.file_type.is_symlink() {
                    self.fs
                        .stat(&crate::fs::vfs_join(&dir, &entry.name))
                        .await
                        .is_ok_and(|m| m.file_type.is_dir())
                } else {
                    false
                };
                if !is_dir {
                    continue;
                }
            }
            names.push(format!("{dir_part}{}", entry.name));
        }
        names.sort();
        Some(names)
    }

    /// Names one `-A` action generates for `word` (prefix match), sorted.
    async fn action_matches(&mut self, action: &str, word: &str) -> Vec<String> {
        let mut names: Vec<String> = match action {
            "file" => return self.path_matches(word, false).await.unwrap_or_default(),
            "directory" => return self.path_matches(word, true).await.unwrap_or_default(),
            "alias" => self.scoped.aliases.keys().cloned().collect(),
            "arrayvar" => self
                .scoped
                .arrays
                .keys()
                .chain(self.scoped.assoc_arrays.keys())
                .cloned()
                .collect(),
            "binding" => super::readline_bind::FUNCTION_NAMES
                .iter()
                .map(|s| s.to_string())
                .collect(),
            // The live registry minus the commands that stand in for
            // programs (`awk`, `grep`): bash's builtins, plus builtins the
            // embedder registered.
            "builtin" | "enabled" => self
                .builtin_names()
                .into_iter()
                .filter(|n| {
                    crate::builtins::BASH_BUILTIN_NAMES.contains(&n.as_str())
                        || self.custom_builtin_names.contains(n)
                        || self
                            .host_builtins
                            .as_ref()
                            .is_some_and(|r| r.lookup(n).is_some())
                })
                .collect(),
            "helptopic" => crate::builtins::BASH_BUILTIN_NAMES
                .iter()
                .map(|s| s.to_string())
                .collect(),
            "command" => {
                let mut v: Vec<String> = self.scoped.aliases.keys().cloned().collect();
                v.extend(KEYWORDS.iter().map(|s| s.to_string()));
                v.extend(self.scoped.functions.keys().cloned());
                v.extend(self.builtin_names());
                v.extend(self.path_executables(word).await);
                v
            }
            "export" => self
                .listable_names()
                .into_iter()
                .filter(|n| {
                    self.var_attrs_get(n).contains(VarAttrs::EXPORT) || self.env.contains_key(n)
                })
                .collect(),
            "function" => self.scoped.functions.keys().cloned().collect(),
            "group" => {
                self.sandbox_names("/etc/group", &self.prompt_user.clone())
                    .await
            }
            "user" => {
                self.sandbox_names("/etc/passwd", &self.prompt_user.clone())
                    .await
            }
            "hostname" => {
                let mut v = vec![self.prompt_host.clone()];
                v.push("localhost".to_string());
                v
            }
            "keyword" => {
                return KEYWORDS
                    .iter()
                    .filter(|k| k.starts_with(word))
                    .map(|s| s.to_string())
                    .collect();
            }
            "setopt" => {
                return crate::builtins::SET_O_OPTIONS
                    .iter()
                    .map(|(n, ..)| n.to_string())
                    .filter(|n| n.starts_with(word))
                    .collect();
            }
            "shopt" => {
                return crate::builtins::SHOPT_OPTIONS
                    .iter()
                    .map(|(n, _)| n.to_string())
                    .filter(|n| n.starts_with(word))
                    .collect();
            }
            "signal" => {
                return SIGNALS
                    .iter()
                    .filter(|s| s.starts_with(word))
                    .map(|s| s.to_string())
                    .collect();
            }
            "variable" => self.listable_names(),
            "job" | "running" | "stopped" => self
                .jobs
                .lock()
                .list()
                .into_iter()
                .map(|j| {
                    j.command
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_string()
                })
                .collect(),
            // `disabled` (no builtin can be disabled here) and `service`.
            _ => Vec::new(),
        };
        names.retain(|n| n.starts_with(word));
        names.sort();
        names.dedup();
        names
    }

    /// Executables on `$PATH` whose names start with `word`.
    async fn path_executables(&mut self, word: &str) -> Vec<String> {
        let path = self.expand_variable("PATH");
        let mut out = Vec::new();
        for dir in path.split(':').filter(|d| !d.is_empty()) {
            let Ok(entries) = self.fs.read_dir(std::path::Path::new(dir)).await else {
                continue;
            };
            for entry in entries {
                if entry.name.starts_with(word)
                    && !entry.metadata.file_type.is_dir()
                    && entry.metadata.mode & 0o111 != 0
                {
                    out.push(entry.name);
                }
            }
        }
        out
    }

    /// First fields of a sandbox `/etc/passwd`-style file, or `fallback`.
    async fn sandbox_names(&self, file: &str, fallback: &str) -> Vec<String> {
        match self.fs.read_file(std::path::Path::new(file)).await {
            Ok(bytes) if bytes.len() <= self.limits.max_input_bytes => {
                String::from_utf8_lossy(&bytes)
                    .lines()
                    .filter_map(|l| l.split(':').next())
                    .filter(|n| !n.is_empty() && !n.starts_with('#'))
                    .map(str::to_string)
                    .collect()
            }
            _ => vec![fallback.to_string()],
        }
    }
}

/// Backslash-escape blanks outside quotes and `$(...)`, so a `-W` word that
/// holds blanks not in IFS parses as one word.
fn escape_blanks(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut quote: Option<char> = None;
    let mut depth = 0usize;
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            out.push(c);
            if c == '\\' && q == '"' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\\' => {
                out.push(c);
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            }
            '\'' | '"' => {
                quote = Some(c);
                out.push(c);
            }
            '(' | '{' => {
                depth += usize::from(out.ends_with('$') || depth > 0);
                out.push(c);
            }
            ')' | '}' if depth > 0 => {
                depth -= 1;
                out.push(c);
            }
            ' ' | '\t' | '\n' if depth == 0 => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out
}

/// A `${` with no closing `}` (bash: bad substitution).
fn has_unclosed_parameter(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == b'$' && bytes[i + 1] == b'{' {
            let mut depth = 0usize;
            let mut j = i + 1;
            let mut closed = false;
            while j < bytes.len() {
                match bytes[j] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            closed = true;
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            if !closed {
                return true;
            }
            i = j;
        }
        i += 1;
    }
    false
}

/// `-X`: an unescaped `&` stands for the word being completed.
fn substitute_ampersand(pattern: &str, word: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'&') => {
                out.push('&');
                chars.next();
            }
            '&' => out.push_str(word),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wordlist_splits_at_ifs_outside_quotes() {
        assert_eq!(
            split_wordlist("spam:eggs%ham cheese\\:colon", ":%"),
            vec!["spam", "eggs", "ham cheese\\:colon"]
        );
        assert_eq!(
            split_wordlist("$(echo \"a:b\") c", ": "),
            vec!["$(echo \"a:b\")", "c"]
        );
    }

    #[test]
    fn spec_prints_like_bash() {
        let parsed = match parse_comp_args(
            "complete",
            &[
                "-o", "nospace", "-v", "-A", "function", "-W", "a b", "-F", "f", "x",
            ]
            .map(String::from),
        ) {
            Ok(p) => p,
            Err(_) => panic!("parse"),
        };
        assert_eq!(
            parsed.spec.print("x"),
            "complete -o nospace -v -A function -W 'a b' -F f x\n"
        );
    }

    #[test]
    fn ampersand_is_the_word() {
        assert_eq!(substitute_ampersand("&b", "a"), "ab");
        assert_eq!(substitute_ampersand("\\&b", "a"), "&b");
    }
}
