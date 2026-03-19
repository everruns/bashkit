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
