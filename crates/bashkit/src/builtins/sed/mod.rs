//! sed - stream editor builtin.
//!
//! Important decisions (issue #2427):
//!
//! - Option parsing is a real getopt: short options cluster (`-ne`), take
//!   attached or separate arguments (`-e`, `-f`, `-l`), `-i` takes an *attached*
//!   suffix only, `--` ends option parsing, and the long spellings are
//!   accepted. Whether the first operand is the script or a file is decided
//!   after the scan, exactly as GNU does.
//! - All script fragments (`-e`, `-f`, the bare operand) are concatenated with
//!   newlines and compiled once, so multi-line `a\` text and `#` comments work.
//! - Without `-s`/`-i` every operand is one continuous stream: line numbers keep
//!   counting, `$` is the last line of the last file, and `q` stops everything.
//! - `-i` rewrites through [`crate::builtins::atomic_write::atomic_replace`],
//!   which preserves the file mode and leaves the original intact on failure
//!   (THREAT[TM-FS-016]), and refuses non-UTF-8 input rather than corrupting it.

mod exec;
mod pattern;
mod script;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;

use exec::{InputLine, Machine};

use super::atomic_write::{AtomicFailpoints, atomic_replace};
use super::{Builtin, Context};
use crate::error::Result;
use crate::fs::vfs_join;
use crate::interpreter::ExecResult;

const HELP: &str = "Usage: sed [OPTION]... {script} [FILE]...\n\
Stream editor for filtering and transforming text.\n\n  \
-n, --quiet, --silent\tsuppress automatic printing of pattern space\n  \
-e script, --expression=script\n\t\tadd the script to the commands to be executed\n  \
-f file, --file=file\tadd the contents of script-file to the commands\n  \
-i[SUFFIX], --in-place[=SUFFIX]\n\t\tedit files in place (makes backup if SUFFIX supplied)\n  \
-E, -r, --regexp-extended\n\t\tuse extended regular expressions\n  \
-s, --separate\tconsider files as separate rather than as a single stream\n  \
-z, --null-data\tseparate lines by NUL characters\n  \
-l N, --line-length=N\tspecify the desired line-wrap length for the `l' command\n  \
-u, --unbuffered\taccepted for compatibility (no effect)\n  \
--help\t\tdisplay this help and exit\n  \
--version\toutput version information and exit\n";

/// The `l` command wraps here unless `-l` or an explicit argument says otherwise.
const DEFAULT_LINE_LENGTH: usize = 70;

/// sed command - stream editor
pub struct Sed;

enum ScriptSource {
    Text(String),
    File(String),
}

struct Options {
    sources: Vec<ScriptSource>,
    files: Vec<String>,
    /// `Some(suffix)`; an empty suffix means no backup copy.
    in_place: Option<String>,
    quiet: bool,
    extended: bool,
    separate: bool,
    null_data: bool,
    line_length: usize,
}

fn usage_error(message: impl std::fmt::Display) -> Box<ExecResult> {
    Box::new(ExecResult::err(format!("sed: {message}\n"), 1))
}

impl Options {
    fn parse(args: &[String]) -> std::result::Result<Options, Box<ExecResult>> {
        let mut opts = Options {
            sources: Vec::new(),
            files: Vec::new(),
            in_place: None,
            quiet: false,
            extended: false,
            separate: false,
            null_data: false,
            line_length: DEFAULT_LINE_LENGTH,
        };
        let mut operands: Vec<String> = Vec::new();
        let mut end_of_options = false;
        let mut i = 0;

        while i < args.len() {
            let arg = &args[i];
            if end_of_options || arg == "-" || !arg.starts_with('-') {
                operands.push(arg.clone());
                i += 1;
                continue;
            }
            if arg == "--" {
                end_of_options = true;
                i += 1;
                continue;
            }
            if let Some(long) = arg.strip_prefix("--") {
                let (name, value) = match long.split_once('=') {
                    Some((n, v)) => (n, Some(v.to_string())),
                    None => (long, None),
                };
                match name {
                    "quiet" | "silent" => opts.quiet = true,
                    "regexp-extended" => opts.extended = true,
                    "separate" => opts.separate = true,
                    "null-data" | "zero-terminated" => opts.null_data = true,
                    "unbuffered" | "debug" | "posix" | "sandbox" | "follow-symlinks" => {}
                    "in-place" => opts.in_place = Some(value.unwrap_or_default()),
                    "expression" => {
                        let value = take_value(args, &mut i, value, arg)?;
                        opts.sources.push(ScriptSource::Text(value));
                    }
                    "file" => {
                        let value = take_value(args, &mut i, value, arg)?;
                        opts.sources.push(ScriptSource::File(value));
                    }
                    "line-length" => {
                        let value = take_value(args, &mut i, value, arg)?;
                        opts.line_length = parse_length(&value)?;
                    }
                    _ => return Err(Box::new(super::invalid_option("sed", arg, 1))),
                }
                i += 1;
                continue;
            }

            let chars: Vec<char> = arg.chars().skip(1).collect();
            let mut j = 0;
            while j < chars.len() {
                match chars[j] {
                    'n' => opts.quiet = true,
                    'r' | 'E' => opts.extended = true,
                    's' => opts.separate = true,
                    'z' => opts.null_data = true,
                    'u' => {}
                    'e' | 'f' | 'l' => {
                        let flag = chars[j];
                        let attached: String = chars[j + 1..].iter().collect();
                        let value = if attached.is_empty() {
                            i += 1;
                            match args.get(i) {
                                Some(v) => v.clone(),
                                None => {
                                    return Err(usage_error(format!(
                                        "option requires an argument -- '{flag}'"
                                    )));
                                }
                            }
                        } else {
                            attached
                        };
                        match flag {
                            'e' => opts.sources.push(ScriptSource::Text(value)),
                            'f' => opts.sources.push(ScriptSource::File(value)),
                            _ => opts.line_length = parse_length(&value)?,
                        }
                        j = chars.len();
                        continue;
                    }
                    'i' => {
                        // GNU only accepts an *attached* suffix, so that
                        // `sed -i 's/a/b/' f` edits f rather than creating `s/a/b/`.
                        opts.in_place = Some(chars[j + 1..].iter().collect());
                        j = chars.len();
                        continue;
                    }
                    _ => {
                        let token = format!("-{}", chars[j]);
                        return Err(Box::new(super::invalid_option("sed", &token, 1)));
                    }
                }
                j += 1;
            }
            i += 1;
        }

        // GNU decides script-vs-file only after the whole scan: any -e/-f makes
        // every operand a file name.
        let mut operands = operands.into_iter();
        if opts.sources.is_empty() {
            match operands.next() {
                Some(script) => opts.sources.push(ScriptSource::Text(script)),
                None => {
                    return Err(Box::new(ExecResult::err(
                        format!("Usage: sed [OPTION]... {{script}} [FILE]...\n{HELP}"),
                        1,
                    )));
                }
            }
        }
        opts.files.extend(operands);

        if opts.in_place.is_some() {
            if opts.files.is_empty() {
                return Err(Box::new(ExecResult::err(
                    "sed: no input files\n".to_string(),
                    4,
                )));
            }
            // -i implies -s: each file is its own stream.
            opts.separate = true;
        }
        Ok(opts)
    }
}

fn take_value(
    args: &[String],
    i: &mut usize,
    inline: Option<String>,
    arg: &str,
) -> std::result::Result<String, Box<ExecResult>> {
    if let Some(v) = inline {
        return Ok(v);
    }
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| usage_error(format!("option '{arg}' requires an argument")))
}

fn parse_length(value: &str) -> std::result::Result<usize, Box<ExecResult>> {
    value
        .parse::<usize>()
        .map_err(|_| usage_error(format!("invalid line length: {value}")))
}

/// Split input into lines, remembering whether the final line was terminated.
fn split_lines(content: &str, sep: char, file: usize, out: &mut Vec<InputLine>) {
    if content.is_empty() {
        return;
    }
    let mut rest = content;
    while let Some(idx) = rest.find(sep) {
        out.push(InputLine {
            text: rest[..idx].to_string(),
            had_newline: true,
            file,
        });
        rest = &rest[idx + sep.len_utf8()..];
    }
    if !rest.is_empty() {
        out.push(InputLine {
            text: rest.to_string(),
            had_newline: false,
            file,
        });
    }
}

fn resolve(cwd: &std::path::Path, name: &str) -> PathBuf {
    if name.starts_with('/') {
        PathBuf::from(name)
    } else {
        vfs_join(cwd, name)
    }
}

#[async_trait]
impl Builtin for Sed {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(ctx.args, HELP, Some("sed (bashkit) 0.1")) {
            return Ok(r);
        }

        let opts = match Options::parse(ctx.args) {
            Ok(o) => o,
            Err(e) => return Ok(*e),
        };

        // Assemble the whole script before compiling it: GNU joins every -e and
        // -f fragment with newlines and parses the result as one program.
        let mut script = String::new();
        for source in &opts.sources {
            if !script.is_empty() {
                script.push('\n');
            }
            match source {
                ScriptSource::Text(text) => script.push_str(text),
                ScriptSource::File(name) => {
                    if name == "-" {
                        script.push_str(
                            ctx.stdin
                                .map(ToString::to_string)
                                .unwrap_or_default()
                                .as_str(),
                        );
                        continue;
                    }
                    let path = resolve(ctx.cwd, name);
                    match ctx.fs.read_file(&path).await {
                        Ok(bytes) => {
                            script.push_str(String::from_utf8_lossy(&bytes).trim_end_matches('\n'))
                        }
                        // GNU exits 4 when a script file cannot be opened.
                        Err(e) => {
                            return Ok(ExecResult::err(
                                format!("sed: couldn't open file {name}: {e}\n"),
                                4,
                            ));
                        }
                    }
                }
            }
        }

        let program = match script::parse(&script, opts.extended) {
            Ok(p) => p,
            Err(e) => return Ok(ExecResult::err(format!("sed: {}\n", e.message), e.code)),
        };

        let sep = if opts.null_data { '\0' } else { '\n' };

        // `r`/`R` operands are read up front; a missing file is silently ignored
        // exactly as in GNU sed.
        let mut read_files: HashMap<String, String> = HashMap::new();
        for name in &program.read_files {
            if read_files.contains_key(name) {
                continue;
            }
            let text = if name == "/dev/stdin" || name == "-" {
                ctx.stdin.map(ToString::to_string).unwrap_or_default()
            } else {
                let path = resolve(ctx.cwd, name);
                match ctx.fs.read_file(&path).await {
                    Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                    Err(_) => continue,
                }
            };
            read_files.insert(name.clone(), text);
        }

        // Gather input.
        let names: Vec<String> = if opts.files.is_empty() {
            vec!["-".to_string()]
        } else {
            opts.files.clone()
        };
        let mut lines: Vec<InputLine> = Vec::new();
        let mut raw: Vec<String> = Vec::new();
        for (index, name) in names.iter().enumerate() {
            let content = if name == "-" || opts.files.is_empty() {
                ctx.stdin.map(ToString::to_string).unwrap_or_default()
            } else {
                let path = resolve(ctx.cwd, name);
                match ctx.fs.read_file(&path).await {
                    Ok(bytes) => match String::from_utf8(bytes) {
                        Ok(text) => text,
                        Err(e) => {
                            if opts.in_place.is_some() {
                                return Ok(ExecResult::err(
                                    format!(
                                        "sed: couldn't edit {name}: not a valid UTF-8 text file\n"
                                    ),
                                    1,
                                ));
                            }
                            String::from_utf8_lossy(e.as_bytes()).into_owned()
                        }
                    },
                    // GNU exits 2 when an input file cannot be read.
                    Err(e) => {
                        return Ok(ExecResult::err(format!("sed: can't read {name}: {e}\n"), 2));
                    }
                }
            };
            split_lines(&content, sep, index, &mut lines);
            raw.push(content);
        }

        // Segment the stream: one segment overall, or one per operand under
        // `-s`/`-i`. Built before the machine so it can borrow the slices.
        let segments: Vec<Vec<InputLine>> = if opts.separate {
            (0..names.len())
                .map(|index| {
                    lines
                        .iter()
                        .filter(|l| l.file == index)
                        .map(|l| InputLine {
                            text: l.text.clone(),
                            had_newline: l.had_newline,
                            file: l.file,
                        })
                        .collect()
                })
                .collect()
        } else {
            vec![lines]
        };

        let mut machine = Machine::new(
            &program,
            &names,
            &read_files,
            opts.quiet,
            opts.line_length,
            sep,
        );

        let mut stdout = String::new();
        let mut edits: Vec<(usize, String, String)> = Vec::new();

        for (index, segment) in segments.iter().enumerate() {
            if machine.finished() {
                break;
            }
            let produced = machine.run_segment(segment);
            let name = names.get(index).map(String::as_str).unwrap_or("-");
            if opts.separate && opts.in_place.is_some() && name != "-" {
                edits.push((index, name.to_string(), produced));
            } else {
                stdout.push_str(&produced);
            }
        }

        let mut stderr = std::mem::take(&mut machine.stderr);
        let exit_code = machine.exit_code.unwrap_or(0);
        let write_files = std::mem::take(&mut machine.write_files);
        drop(machine);

        // Files named by `w`/`W`/`s///w`.
        for (name, sink) in write_files {
            let path = resolve(ctx.cwd, &name);
            if let Err(e) = ctx.fs.write_file(&path, sink.buf.as_bytes()).await {
                return Ok(ExecResult::err(
                    format!("sed: couldn't open {name}: {e}\n"),
                    4,
                ));
            }
        }

        // In-place rewrites.
        for (index, name, content) in edits {
            let path = resolve(ctx.cwd, &name);
            if let Some(suffix) = opts.in_place.as_deref().filter(|s| !s.is_empty()) {
                let backup = if suffix.contains('*') {
                    suffix.replace('*', &name)
                } else {
                    format!("{name}{suffix}")
                };
                let backup_path = resolve(ctx.cwd, &backup);
                let original = raw.get(index).cloned().unwrap_or_default();
                if let Err(e) = ctx.fs.write_file(&backup_path, original.as_bytes()).await {
                    return Ok(ExecResult::err(
                        format!("sed: cannot rename {name}: {e}\n"),
                        4,
                    ));
                }
            }
            if let Err(e) = atomic_replace(
                &*ctx.fs,
                &path,
                content.as_bytes(),
                "sed",
                AtomicFailpoints {
                    allocate: "sed::temp_allocate",
                    chmod: "sed::temp_chmod",
                    rename: "sed::temp_rename",
                },
            )
            .await
            {
                return Ok(ExecResult::err(format!("sed: {name}: {e}\n"), 4));
            }
        }

        if stderr.len() > 1024 {
            stderr.truncate(1024);
        }
        Ok(ExecResult {
            stdout: stdout.into(),
            stderr: stderr.into(),
            exit_code,
            ..Default::default()
        })
    }
}
