// Fuzz-target input policies live here so their security boundaries are unit tested.

/// Returns whether `input` is a self-contained Bash arithmetic expression.
///
/// The arithmetic fuzzer embeds accepted input inside `$((...))`. Restricting
/// the alphabet and requiring balanced grouping prevents input from escaping
/// that expansion and turning the focused target into a general shell fuzzer.
pub fn is_arithmetic_expression(input: &str) -> bool {
    if input.is_empty() {
        return false;
    }

    const MAX_GROUPING_DEPTH: usize = 20;
    let mut grouping = [0_u8; MAX_GROUPING_DEPTH];
    let mut depth = 0;

    for byte in input.bytes() {
        match byte {
            b'0'..=b'9'
            | b'a'..=b'z'
            | b'A'..=b'Z'
            | b'_'
            | b' '
            | b'\t'
            | b'+'
            | b'-'
            | b'*'
            | b'/'
            | b'%'
            | b'<'
            | b'>'
            | b'='
            | b'!'
            | b'&'
            | b'|'
            | b'^'
            | b'~'
            | b'?'
            | b':'
            | b','
            | b'#' => {}
            b'(' | b'[' if depth < MAX_GROUPING_DEPTH => {
                grouping[depth] = byte;
                depth += 1;
            }
            b')' if depth > 0 && grouping[depth - 1] == b'(' => depth -= 1,
            b']' if depth > 0 && grouping[depth - 1] == b'[' => depth -= 1,
            _ => return false,
        }
    }

    depth == 0
}

#[cfg(test)]
mod tests {
    use super::is_arithmetic_expression;

    #[test]
    fn accepts_arithmetic_language() {
        for expression in [
            "1 + 2 * 3",
            "(value << 2) | 0xff",
            "array[index + 1]",
            "total += condition ? 10 : 2",
            "16#ff + 2#1010",
            "++counter, mask & ~flag",
        ] {
            assert!(is_arithmetic_expression(expression), "{expression:?}");
        }
    }

    #[test]
    fn rejects_shell_syntax_and_unbalanced_grouping() {
        for expression in [
            "",
            "))/.rustup/toolchains/",
            "1)) ; uname",
            "$(uname)",
            "`uname`",
            "1\necho escaped",
            "array[index",
            "(1 + 2",
            "1 + 2)",
            "([1)]",
            "(((((((((((((((((((((1)))))))))))))))))))))",
        ] {
            assert!(!is_arithmetic_expression(expression), "{expression:?}");
        }
    }
}
