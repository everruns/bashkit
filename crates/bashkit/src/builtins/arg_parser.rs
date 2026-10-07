// Shared arg-parsing utility to replace manual `while i < args.len()` loops.
//
// Design decision: struct with `flag()` and `flag_value()` methods that
// handle both `-fVALUE` (attached) and `-f VALUE` (next arg) forms.
// Each method advances the internal position, so the caller doesn't
// manage index arithmetic. Positional args are consumed with `positional()`.

/// Shared argument parser for builtins.
///
/// Replaces the common `while i < args.len()` pattern with a cleaner API.
///
/// # Usage
///
/// ```rust,ignore
/// let mut parser = ArgParser::new(args);
/// while !parser.is_done() {
///     if parser.flag("-v") {
///         verbose = true;
///     } else if let Some(val) = parser.flag_value("-n", "cmd")? {
///         count = val.parse().map_err(|_| format!("cmd: invalid number: '{val}'"))?;
///     } else {
///         files.push(parser.positional().unwrap().to_string());
///     }
/// }
/// ```
pub(crate) struct ArgParser<'a> {
    args: &'a [String],
    pos: usize,
}

impl<'a> ArgParser<'a> {
    pub fn new(args: &'a [String]) -> Self {
        Self { args, pos: 0 }
    }

    /// Returns true if all args have been consumed.
    pub fn is_done(&self) -> bool {
        self.pos >= self.args.len()
    }

    /// Peek at current arg without advancing.
    pub fn current(&self) -> Option<&'a str> {
        self.args.get(self.pos).map(|s| s.as_str())
    }

    /// Returns remaining args as a slice (from current position).
    pub fn rest(&self) -> &'a [String] {
        if self.pos < self.args.len() {
            &self.args[self.pos..]
        } else {
            &[]
        }
    }

    /// Advance past current arg.
    pub fn advance(&mut self) {
        self.pos += 1;
    }

    /// Try to consume a boolean flag (exact match). Advances if matched.
    pub fn flag(&mut self, name: &str) -> bool {
        if self.current() == Some(name) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Try to consume any of several boolean flag names. Advances if matched.
    pub fn flag_any(&mut self, names: &[&str]) -> bool {
        if self.current().is_some_and(|cur| names.contains(&cur)) {
            self.advance();
            return true;
        }
        false
    }

    /// Try to consume a flag with a required value.
    ///
    /// Handles both `-fVALUE` (attached) and `-f VALUE` (next arg) forms.
    /// Returns `Ok(Some(value))` if matched, `Err` if matched but no value,
    /// `Ok(None)` if current arg doesn't match.
    /// Advances past consumed args on success.
    pub fn flag_value(
        &mut self,
        name: &str,
        cmd: &str,
    ) -> std::result::Result<Option<&'a str>, String> {
        let arg = match self.args.get(self.pos) {
            Some(a) => a.as_str(),
            None => return Ok(None),
        };

        if arg == name {
            // Exact match: value is next arg
            self.pos += 1;
            match self.args.get(self.pos) {
                Some(val) => {
                    self.pos += 1;
                    Ok(Some(val.as_str()))
                }
                None => Err(format!("{cmd}: {name} requires an argument")),
            }
        } else if let Some(rest) = arg.strip_prefix(name) {
            // Attached form: -nVALUE
            if !rest.is_empty() {
                self.pos += 1;
                Ok(Some(rest))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    /// Like `flag_value` but for multiple flag names (e.g. `-o` and `--output`).
    /// Only the first name supports the attached `-oVALUE` form.
    pub fn flag_value_any(
        &mut self,
        names: &[&str],
        cmd: &str,
    ) -> std::result::Result<Option<&'a str>, String> {
        let arg = match self.args.get(self.pos) {
            Some(a) => a.as_str(),
            None => return Ok(None),
        };

        for (i, &name) in names.iter().enumerate() {
            if arg == name {
                self.pos += 1;
                return match self.args.get(self.pos) {
                    Some(val) => {
                        self.pos += 1;
                        Ok(Some(val.as_str()))
                    }
                    None => Err(format!("{cmd}: {name} requires an argument")),
                };
            }
            // Only try attached form for short flags (first name typically)
            if i == 0
                && let Some(rest) = arg.strip_prefix(name).filter(|r| !r.is_empty())
            {
                self.pos += 1;
                return Ok(Some(rest));
            }
        }

        Ok(None)
    }

    /// Try to consume a long option with a value, handling both
    /// `--name=value` and `--name value` forms.
    ///
    /// Unlike [`flag_value`](Self::flag_value), the attached form requires an
    /// explicit `=` separator (GNU long-option convention), so `--max-procs4`
    /// is not mistaken for `--max-procs=4`. Returns `Ok(Some(value))` if
    /// matched, `Err` if matched but no value follows, `Ok(None)` otherwise.
    pub fn long_value(
        &mut self,
        name: &str,
        cmd: &str,
    ) -> std::result::Result<Option<&'a str>, String> {
        let arg = match self.args.get(self.pos) {
            Some(a) => a.as_str(),
            None => return Ok(None),
        };

        if arg == name {
            // Separate form: --name value
            self.pos += 1;
            return match self.args.get(self.pos) {
                Some(val) => {
                    self.pos += 1;
                    Ok(Some(val.as_str()))
                }
                None => Err(format!("{cmd}: {name} requires an argument")),
            };
        }

        // Attached form: --name=value
        if let Some(rest) = arg.strip_prefix(name)
            && let Some(val) = rest.strip_prefix('=')
        {
            self.pos += 1;
            return Ok(Some(val));
        }

        Ok(None)
    }

    /// Try to consume a flag with a value, silently returning None if
    /// the flag matches but no value is available (for lenient parsers).
    pub fn flag_value_opt(&mut self, name: &str) -> Option<&'a str> {
        let arg = match self.args.get(self.pos) {
            Some(a) => a.as_str(),
            None => return None,
        };

        if arg == name {
            self.pos += 1;
            if let Some(val) = self.args.get(self.pos) {
                self.pos += 1;
                Some(val.as_str())
            } else {
                None
            }
        } else if let Some(rest) = arg.strip_prefix(name) {
            if !rest.is_empty() {
                self.pos += 1;
                Some(rest)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Consume current arg as a positional argument. Returns None if done.
    pub fn positional(&mut self) -> Option<&'a str> {
        let val = self.args.get(self.pos).map(|s| s.as_str())?;
        self.pos += 1;
        Some(val)
    }

    /// Check if current arg looks like a flag (starts with `-`, length > 1).
    pub fn is_flag(&self) -> bool {
        self.args
            .get(self.pos)
            .map(|s| s.starts_with('-') && s.len() > 1)
            .unwrap_or(false)
    }
}

// Decision: `gnu_getopt` mirrors glibc `getopt_long` for builtins whose GNU
// counterparts accept bundled short options (`-sf1`), attached or separate
// values, `--long=VALUE`, unambiguous long prefixes, and (optionally)
// options after operands. Long options map onto a key char so callers
// handle `-d X` and `--delimiter=X` in one match arm.

/// Whether an option takes an argument.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum OptArg {
    No,
    Required,
    /// Only attached: `-xVAL` / `--long=VAL`.
    Optional,
}

/// One parsed option occurrence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParsedOpt {
    pub key: char,
    pub value: Option<String>,
}

/// Parse `args` GNU-style.
///
/// `short` uses getopt syntax: a letter, `:` after it for a required
/// value, `::` for an optional attached value. `longs` lists
/// `(name, arg, key)`. With `permute`, operands and options may interleave
/// (GNU default); without it, the first operand ends option parsing.
/// Errors are GNU-shaped messages with `code` as the exit status.
pub(crate) fn gnu_getopt(
    cmd: &str,
    args: &[String],
    short: &str,
    longs: &[(&str, OptArg, char)],
    permute: bool,
    code: i32,
) -> std::result::Result<(Vec<ParsedOpt>, Vec<String>), crate::interpreter::ExecResult> {
    use crate::interpreter::ExecResult;
    let err = |msg: String| ExecResult::err(format!("{cmd}: {msg}\n"), code);
    let short_arg = |c: char| -> Option<OptArg> {
        let idx = short.find(c)?;
        if c == ':' {
            return None;
        }
        let rest = &short[idx + c.len_utf8()..];
        Some(if rest.starts_with("::") {
            OptArg::Optional
        } else if rest.starts_with(':') {
            OptArg::Required
        } else {
            OptArg::No
        })
    };

    let mut opts = Vec::new();
    let mut operands = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        i += 1;
        if arg == "--" {
            operands.extend(args[i..].iter().cloned());
            break;
        }
        if arg == "-" || !arg.starts_with('-') {
            operands.push(arg.clone());
            if !permute {
                operands.extend(args[i..].iter().cloned());
                break;
            }
            continue;
        }
        if let Some(long) = arg.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            let hits: Vec<&(&str, OptArg, char)> = match longs.iter().find(|l| l.0 == name) {
                Some(l) => vec![l],
                None => longs.iter().filter(|l| l.0.starts_with(name)).collect(),
            };
            let &(lname, kind, key) = match hits.as_slice() {
                [one] => *one,
                [] => return Err(err(format!("unrecognized option '--{name}'"))),
                many => {
                    // Prefixes naming the same key are not ambiguous.
                    if many.iter().all(|l| l.2 == many[0].2 && l.1 == many[0].1) {
                        many[0]
                    } else {
                        return Err(err(format!("option '--{name}' is ambiguous")));
                    }
                }
            };
            let value = match (kind, inline) {
                (OptArg::No, Some(_)) => {
                    return Err(err(format!("option '--{lname}' doesn't allow an argument")));
                }
                (OptArg::No, None) | (OptArg::Optional, None) => None,
                (_, Some(v)) => Some(v),
                (OptArg::Required, None) => {
                    if i < args.len() {
                        i += 1;
                        Some(args[i - 1].clone())
                    } else {
                        return Err(err(format!("option '--{lname}' requires an argument")));
                    }
                }
            };
            opts.push(ParsedOpt { key, value });
            continue;
        }
        let body = &arg[1..];
        for (pos, c) in body.char_indices() {
            let Some(kind) = short_arg(c) else {
                return Err(err(format!("invalid option -- '{c}'")));
            };
            let attached = &body[pos + c.len_utf8()..];
            match kind {
                OptArg::No => opts.push(ParsedOpt {
                    key: c,
                    value: None,
                }),
                OptArg::Optional => {
                    opts.push(ParsedOpt {
                        key: c,
                        value: (!attached.is_empty()).then(|| attached.to_string()),
                    });
                    break;
                }
                OptArg::Required => {
                    let value = if !attached.is_empty() {
                        attached.to_string()
                    } else if i < args.len() {
                        i += 1;
                        args[i - 1].clone()
                    } else {
                        return Err(err(format!("option requires an argument -- '{c}'")));
                    };
                    opts.push(ParsedOpt {
                        key: c,
                        value: Some(value),
                    });
                    break;
                }
            }
        }
    }
    Ok((opts, operands))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn args(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_flag() {
        let a = args(&["-v", "file"]);
        let mut p = ArgParser::new(&a);
        assert!(p.flag("-v"));
        assert!(!p.flag("-v"));
        assert_eq!(p.current(), Some("file"));
    }

    #[test]
    fn test_flag_value_separate() {
        let a = args(&["-n", "10", "file"]);
        let mut p = ArgParser::new(&a);
        assert_eq!(p.flag_value("-n", "cmd").unwrap(), Some("10"));
        assert_eq!(p.current(), Some("file"));
    }

    #[test]
    fn test_flag_value_attached() {
        let a = args(&["-n10", "file"]);
        let mut p = ArgParser::new(&a);
        assert_eq!(p.flag_value("-n", "cmd").unwrap(), Some("10"));
        assert_eq!(p.current(), Some("file"));
    }

    #[test]
    fn test_flag_value_missing() {
        let a = args(&["-n"]);
        let mut p = ArgParser::new(&a);
        assert!(p.flag_value("-n", "cmd").is_err());
    }

    #[test]
    fn test_flag_value_no_match() {
        let a = args(&["-v"]);
        let mut p = ArgParser::new(&a);
        assert_eq!(p.flag_value("-n", "cmd").unwrap(), None);
        // Position unchanged
        assert_eq!(p.current(), Some("-v"));
    }

    #[test]
    fn test_flag_any() {
        let a = args(&["--verbose"]);
        let mut p = ArgParser::new(&a);
        assert!(p.flag_any(&["-v", "--verbose"]));
        assert!(p.is_done());
    }

    #[test]
    fn test_flag_value_any() {
        let a = args(&["--output", "file.txt"]);
        let mut p = ArgParser::new(&a);
        assert_eq!(
            p.flag_value_any(&["-o", "--output"], "cmd").unwrap(),
            Some("file.txt")
        );
    }

    #[test]
    fn test_flag_value_opt_no_value() {
        let a = args(&["-n"]);
        let mut p = ArgParser::new(&a);
        // No value available, returns None without error
        assert_eq!(p.flag_value_opt("-n"), None);
    }

    #[test]
    fn test_flag_value_opt_separate() {
        let a = args(&["-n", "10", "file"]);
        let mut p = ArgParser::new(&a);
        assert_eq!(p.flag_value_opt("-n"), Some("10"));
        assert_eq!(p.current(), Some("file"));
    }

    #[test]
    fn test_flag_value_opt_attached() {
        let a = args(&["-n10", "file"]);
        let mut p = ArgParser::new(&a);
        assert_eq!(p.flag_value_opt("-n"), Some("10"));
        assert_eq!(p.current(), Some("file"));
    }

    #[test]
    fn test_flag_value_any_attached() {
        let a = args(&["-ofile.txt"]);
        let mut p = ArgParser::new(&a);
        assert_eq!(
            p.flag_value_any(&["-o", "--output"], "cmd").unwrap(),
            Some("file.txt")
        );
        assert!(p.is_done());
    }

    #[test]
    fn test_flag_value_any_missing() {
        let a = args(&["--output"]);
        let mut p = ArgParser::new(&a);
        assert!(p.flag_value_any(&["-o", "--output"], "cmd").is_err());
    }

    #[test]
    fn test_long_value_attached() {
        let a = args(&["--max-procs=4", "cmd"]);
        let mut p = ArgParser::new(&a);
        assert_eq!(p.long_value("--max-procs", "xargs").unwrap(), Some("4"));
        assert_eq!(p.current(), Some("cmd"));
    }

    #[test]
    fn test_long_value_separate() {
        let a = args(&["--process-slot-var", "SLOT", "cmd"]);
        let mut p = ArgParser::new(&a);
        assert_eq!(
            p.long_value("--process-slot-var", "xargs").unwrap(),
            Some("SLOT")
        );
        assert_eq!(p.current(), Some("cmd"));
    }

    #[test]
    fn test_long_value_no_match() {
        let a = args(&["-P", "4"]);
        let mut p = ArgParser::new(&a);
        assert_eq!(p.long_value("--max-procs", "xargs").unwrap(), None);
        assert_eq!(p.current(), Some("-P"));
    }

    #[test]
    fn test_long_value_missing() {
        let a = args(&["--max-procs"]);
        let mut p = ArgParser::new(&a);
        assert!(p.long_value("--max-procs", "xargs").is_err());
    }

    #[test]
    fn test_long_value_attached_empty() {
        // `--max-procs=` yields an empty value, not None.
        let a = args(&["--max-procs="]);
        let mut p = ArgParser::new(&a);
        assert_eq!(p.long_value("--max-procs", "xargs").unwrap(), Some(""));
    }

    #[test]
    fn test_current() {
        let a = args(&["hello"]);
        let mut p = ArgParser::new(&a);
        assert_eq!(p.current(), Some("hello"));
        p.advance();
        assert_eq!(p.current(), None);
    }

    #[test]
    fn test_positional() {
        let a = args(&["file1", "file2"]);
        let mut p = ArgParser::new(&a);
        assert_eq!(p.positional(), Some("file1"));
        assert_eq!(p.positional(), Some("file2"));
        assert!(p.is_done());
    }

    #[test]
    fn test_rest() {
        let a = args(&["-v", "cmd", "arg1", "arg2"]);
        let mut p = ArgParser::new(&a);
        p.advance(); // skip -v
        p.advance(); // skip cmd
        assert_eq!(p.rest().len(), 2);
    }

    fn getopt(a: &[&str], permute: bool) -> (Vec<(char, Option<String>)>, Vec<String>) {
        let (opts, ops) = gnu_getopt(
            "t",
            &args(a),
            "ab:c::",
            &[
                ("alpha", OptArg::No, 'a'),
                ("bravo", OptArg::Required, 'b'),
                ("charlie", OptArg::Optional, 'c'),
            ],
            permute,
            1,
        )
        .unwrap();
        (opts.into_iter().map(|o| (o.key, o.value)).collect(), ops)
    }

    #[test]
    fn test_gnu_getopt_bundles_and_values() {
        let (o, ops) = getopt(&["-ab1", "-b", "2", "-cX", "-c", "f"], true);
        assert_eq!(
            o,
            vec![
                ('a', None),
                ('b', Some("1".into())),
                ('b', Some("2".into())),
                ('c', Some("X".into())),
                ('c', None)
            ]
        );
        assert_eq!(ops, vec!["f".to_string()]);
    }

    #[test]
    fn test_gnu_getopt_long_prefix_and_permute() {
        let (o, ops) = getopt(&["x", "--br=v", "--alp", "--", "-a"], true);
        assert_eq!(o, vec![('b', Some("v".into())), ('a', None)]);
        assert_eq!(ops, vec!["x".to_string(), "-a".to_string()]);
        let (o, ops) = getopt(&["x", "-a"], false);
        assert!(o.is_empty());
        assert_eq!(ops, vec!["x".to_string(), "-a".to_string()]);
    }

    #[test]
    fn test_gnu_getopt_errors() {
        let e = gnu_getopt("t", &args(&["-z"]), "a", &[], true, 2).unwrap_err();
        assert_eq!(e.stderr, "t: invalid option -- 'z'\n");
        assert_eq!(e.exit_code, 2);
        let e = gnu_getopt("t", &args(&["-b"]), "b:", &[], true, 1).unwrap_err();
        assert!(e.stderr.contains("requires an argument"));
        let e = gnu_getopt("t", &args(&["--nope"]), "", &[], true, 1).unwrap_err();
        assert!(e.stderr.contains("unrecognized option '--nope'"));
    }

    #[test]
    fn test_is_flag() {
        let a = args(&["-v", "-", "file", "--long"]);
        let mut p = ArgParser::new(&a);
        assert!(p.is_flag()); // -v
        p.advance();
        assert!(!p.is_flag()); // - (single dash)
        p.advance();
        assert!(!p.is_flag()); // file
        p.advance();
        assert!(p.is_flag()); // --long
    }
}
