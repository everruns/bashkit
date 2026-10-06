//! cmp builtin - compare two files byte by byte

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// `cmp` builtin.
///
/// Usage: cmp [-l] [-s] [-b] [-n LIMIT] [-i SKIP] FILE1 [FILE2]
///
/// Exit 0 when identical, 1 when different, 2 on trouble (GNU semantics).
pub struct Cmp;

const HELP: &str = "Usage: cmp [OPTION]... FILE1 [FILE2]\nCompare two files byte by byte.\n\n  -b, --print-bytes\tprint differing bytes\n  -i, --ignore-initial=SKIP\tskip first SKIP bytes of both inputs\n  -l, --verbose\toutput byte numbers and differing byte values\n  -n, --bytes=LIMIT\tcompare at most LIMIT bytes\n  -s, --quiet, --silent\tsuppress all normal output\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n";

fn trouble(msg: String) -> ExecResult {
    ExecResult::err(format!("cmp: {msg}\n"), 2)
}

fn printable(b: u8) -> String {
    match b {
        0..=31 => format!("^{}", (b + 64) as char),
        127 => "^?".to_string(),
        128.. => {
            let low = b & 0x7f;
            match low {
                0..=31 => format!("M-^{}", (low + 64) as char),
                127 => "M-^?".to_string(),
                _ => format!("M-{}", low as char),
            }
        }
        _ => (b as char).to_string(),
    }
}

#[async_trait]
impl Builtin for Cmp {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(ctx.args, HELP, Some("cmp (bashkit) 0.1")) {
            return Ok(r);
        }
        let (mut verbose, mut silent, mut print_bytes) = (false, false, false);
        let mut limit: Option<usize> = None;
        let mut skip = 0usize;
        let mut files: Vec<&str> = Vec::new();
        let mut args = ctx.args.iter();
        let mut only_files = false;
        while let Some(arg) = args.next() {
            let a = arg.as_str();
            if only_files || a == "-" || !a.starts_with('-') {
                files.push(a);
                continue;
            }
            let (key, inline) = match a.split_once('=') {
                Some((k, v)) if a.starts_with("--") => (k, Some(v)),
                _ => (a, None),
            };
            let mut value = |name: &str| -> std::result::Result<usize, String> {
                let v = inline
                    .map(str::to_string)
                    .or_else(|| args.next().cloned())
                    .ok_or_else(|| format!("option requires an argument -- '{name}'"))?;
                v.parse()
                    .map_err(|_| format!("invalid --{name} value '{v}'"))
            };
            match key {
                "--" => only_files = true,
                "-l" | "--verbose" => verbose = true,
                "-s" | "--quiet" | "--silent" => silent = true,
                "-b" | "--print-bytes" => print_bytes = true,
                "-n" | "--bytes" => match value("bytes") {
                    Ok(v) => limit = Some(v),
                    Err(e) => return Ok(trouble(e)),
                },
                "-i" | "--ignore-initial" => match value("ignore-initial") {
                    Ok(v) => skip = v,
                    Err(e) => return Ok(trouble(e)),
                },
                _ if a.len() > 2 && !a.starts_with("--") => {
                    for c in a[1..].chars() {
                        match c {
                            'l' => verbose = true,
                            's' => silent = true,
                            'b' => print_bytes = true,
                            _ => return Ok(trouble(format!("invalid option -- '{c}'"))),
                        }
                    }
                }
                _ => return Ok(trouble(format!("unrecognized option '{a}'"))),
            }
        }
        if files.is_empty() || files.len() > 2 {
            return Ok(trouble(if files.is_empty() {
                "missing operand after 'cmp'".to_string()
            } else {
                format!("invalid --ignore-initial value '{}'", files[2])
            }));
        }
        let name2 = files.get(1).copied().unwrap_or("-");
        let mut data = Vec::with_capacity(2);
        for name in [files[0], name2] {
            if name == "-" {
                data.push(ctx.stdin_bytes().unwrap_or_default().to_vec());
                continue;
            }
            let path = super::resolve_path(ctx.cwd, name);
            match ctx.fs.read_file(&path).await {
                Ok(d) => data.push(d),
                Err(_) => return Ok(trouble(format!("{name}: No such file or directory"))),
            }
        }
        let a = data[0].get(skip..).unwrap_or_default();
        let b = data[1].get(skip..).unwrap_or_default();
        let len = |s: &[u8]| limit.map_or(s.len(), |l| s.len().min(l));
        let (a, b) = (&a[..len(a)], &b[..len(b)]);

        let mut out = String::new();
        let mut line = 1usize;
        let mut differ = false;
        for (i, (&x, &y)) in a.iter().zip(b.iter()).enumerate() {
            if x != y {
                differ = true;
                if silent {
                    break;
                }
                if verbose {
                    out.push_str(&format!("{} {:o} {:o}\n", i + 1, x, y));
                    continue;
                }
                out.push_str(&format!(
                    "{} {} differ: {} {}, line {}",
                    files[0],
                    name2,
                    if print_bytes { "byte" } else { "char" },
                    i + 1,
                    line
                ));
                if print_bytes {
                    out.push_str(&format!(
                        " is {:3o} {} {:3o} {}",
                        x,
                        printable(x),
                        y,
                        printable(y)
                    ));
                }
                out.push('\n');
                break;
            }
            if x == b'\n' {
                line += 1;
            }
        }
        let mut err = String::new();
        if a.len() != b.len() && (!differ || verbose) {
            differ = true;
            if !silent {
                let (short, data) = if a.len() < b.len() {
                    (files[0], a)
                } else {
                    (name2, b)
                };
                let n = data.len();
                let newlines = data.iter().filter(|&&c| c == b'\n').count();
                if n == 0 {
                    err.push_str(&format!("cmp: EOF on {short} which is empty\n"));
                } else if verbose {
                    err.push_str(&format!("cmp: EOF on {short} after byte {n}\n"));
                } else if data.ends_with(b"\n") {
                    err.push_str(&format!(
                        "cmp: EOF on {short} after byte {n}, line {newlines}\n"
                    ));
                } else {
                    err.push_str(&format!(
                        "cmp: EOF on {short} after byte {n}, in line {}\n",
                        newlines + 1
                    ));
                }
            }
        }
        Ok(ExecResult {
            stdout: out.into(),
            stderr: err.into(),
            exit_code: i32::from(differ),
            ..Default::default()
        })
    }
}
