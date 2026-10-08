//! mapfile/readarray builtin — read records from stdin into an array.
//!
//! Mutates arrays via [`BuiltinSideEffect::SetIndexedArray`](super::BuiltinSideEffect).
//!
//! Decision: `-u FD` other than 0 and the `-C`/`-c` callback are rejected with
//! status 2 instead of being ignored; reading the wrong input silently would
//! be worse than a clear error (see L-MAPFILE-001 in operations/limitations).

use async_trait::async_trait;

use super::{Builtin, BuiltinSideEffect, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// `mapfile`/`readarray` builtin — read records from stdin into an indexed array.
///
/// Usage: mapfile [-d DELIM] [-n COUNT] [-O ORIGIN] [-s COUNT] [-t] [ARRAY]
///
/// - `-d DELIM` — end records at the first char of DELIM (`''` means NUL)
/// - `-n COUNT` — copy at most COUNT records (0 means all)
/// - `-O ORIGIN` — start at index ORIGIN and keep existing elements
/// - `-s COUNT` — skip the first COUNT records
/// - `-t` — strip the delimiter from each record
/// - Default array name is `MAPFILE`
pub struct Mapfile;

struct Opts {
    delim: char,
    count: usize,
    origin: Option<usize>,
    skip: usize,
    trim: bool,
    name: String,
}

fn parse_count(opt: char, v: &str) -> std::result::Result<usize, String> {
    v.trim().parse::<usize>().map_err(|_| {
        let what = if opt == 'O' { "origin" } else { "line count" };
        format!("{v}: invalid {what}")
    })
}

fn parse_opts(args: &[String]) -> std::result::Result<Opts, String> {
    let mut o = Opts {
        delim: '\n',
        count: 0,
        origin: None,
        skip: 0,
        trim: false,
        name: "MAPFILE".to_string(),
    };
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        i += 1;
        if arg == "--" {
            break;
        }
        if !arg.starts_with('-') || arg == "-" {
            i -= 1;
            break;
        }
        let mut chars = arg[1..].chars();
        while let Some(c) = chars.next() {
            match c {
                't' => o.trim = true,
                'd' | 'n' | 'O' | 's' | 'u' | 'C' | 'c' => {
                    let rest: String = chars.by_ref().collect();
                    let value = if !rest.is_empty() {
                        rest
                    } else if let Some(v) = args.get(i) {
                        i += 1;
                        v.clone()
                    } else {
                        return Err(format!("-{c}: option requires an argument"));
                    };
                    match c {
                        'd' => o.delim = value.chars().next().unwrap_or('\0'),
                        'n' => o.count = parse_count(c, &value)?,
                        'O' => o.origin = Some(parse_count(c, &value)?),
                        's' => o.skip = parse_count(c, &value)?,
                        'u' if value.trim() == "0" => {}
                        'u' => return Err(format!("-u {value}: only fd 0 is supported")),
                        _ => return Err(format!("-{c}: callbacks are not supported")),
                    }
                }
                _ => return Err(format!("-{c}: invalid option")),
            }
        }
    }
    if let Some(name) = args.get(i) {
        o.name = name.clone();
    }
    Ok(o)
}

#[async_trait]
impl Builtin for Mapfile {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let o = match parse_opts(ctx.args) {
            Ok(o) => o,
            Err(msg) => return Ok(ExecResult::err(format!("mapfile: {msg}\n"), 2)),
        };
        let input = ctx.stdin.map(|stdin| &**stdin).unwrap_or("");

        let mut result = ExecResult::ok(String::new());
        // Without -O the array is cleared first.
        if o.origin.is_none() {
            result
                .side_effects
                .push(BuiltinSideEffect::RemoveArray(o.name.clone()));
        }

        let start = o.origin.unwrap_or(0);
        let entries: Vec<(usize, String)> = input
            .split_inclusive(o.delim)
            .skip(o.skip)
            .take(if o.count == 0 { usize::MAX } else { o.count })
            .enumerate()
            .map(|(idx, rec)| {
                let value = if o.trim {
                    rec.strip_suffix(o.delim).unwrap_or(rec)
                } else {
                    rec
                };
                // Elements are C strings in bash: a NUL ends the value.
                let value = value.split('\0').next().unwrap_or_default();
                (start + idx, value.to_string())
            })
            .collect();

        if !entries.is_empty() {
            result
                .side_effects
                .push(BuiltinSideEffect::SetIndexedArray {
                    name: o.name,
                    entries,
                });
        }

        Ok(result)
    }
}
