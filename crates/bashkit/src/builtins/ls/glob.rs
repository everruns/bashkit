//! Glob pattern matching for `find -name` and `ls`.

/// Simple glob pattern matching for find -name.
pub(crate) fn glob_match(value: &str, pattern: &str) -> bool {
    let mut value_chars = value.chars().peekable();
    let mut pattern_chars = pattern.chars().peekable();

    loop {
        match (pattern_chars.peek(), value_chars.peek()) {
            (None, None) => return true,
            (None, Some(_)) => return false,
            (Some('*'), _) => {
                pattern_chars.next();
                if pattern_chars.peek().is_none() {
                    return true;
                }
                while value_chars.peek().is_some() {
                    let remaining_value: String = value_chars.clone().collect();
                    let remaining_pattern: String = pattern_chars.clone().collect();
                    if glob_match(&remaining_value, &remaining_pattern) {
                        return true;
                    }
                    value_chars.next();
                }
                let remaining_pattern: String = pattern_chars.collect();
                return glob_match("", &remaining_pattern);
            }
            (Some('?'), Some(_)) => {
                pattern_chars.next();
                value_chars.next();
            }
            (Some('?'), None) => return false,
            (Some(p), Some(v)) => {
                if *p == *v {
                    pattern_chars.next();
                    value_chars.next();
                } else {
                    return false;
                }
            }
            (Some(_), None) => return false,
        }
    }
}
