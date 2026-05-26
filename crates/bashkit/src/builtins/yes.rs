//! yes builtin - repeatedly output a line

use async_trait::async_trait;

use super::limits::{YES_MAX_LINES as MAX_LINES, YES_MAX_OUTPUT_BYTES as MAX_OUTPUT_BYTES};
use super::{Builtin, BuiltinHelper, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The yes builtin - output a string repeatedly.
///
/// Usage: yes [STRING]
///
/// Repeatedly outputs STRING (default: "y") followed by newline.
/// In bashkit, output is limited to avoid infinite loops.
pub struct Yes;

impl BuiltinHelper for Yes {
    const NAME: &'static str = "yes";
}

fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn build_yes_output(text: &str) -> String {
    let max_text_bytes = MAX_OUTPUT_BYTES.saturating_sub(1);
    let line_text = truncate_to_char_boundary(text, max_text_bytes);
    let bytes_per_line = line_text.len() + 1; // newline
    let max_lines_by_bytes = (MAX_OUTPUT_BYTES / bytes_per_line).max(1);
    let line_count = MAX_LINES.min(max_lines_by_bytes);

    let mut output = String::with_capacity(bytes_per_line * line_count);
    for _ in 0..line_count {
        output.push_str(line_text);
        output.push('\n');
    }
    output
}

#[async_trait]
impl Builtin for Yes {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = Self::check_help(
            ctx.args,
            "Usage: yes [STRING]\nRepeatedly output a line with STRING, or 'y'.\n\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("yes (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let text = if ctx.args.is_empty() {
            "y".to_string()
        } else {
            ctx.args.join(" ")
        };

        Ok(ExecResult::ok(build_yes_output(&text)))
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_OUTPUT_BYTES, build_yes_output};

    #[test]
    fn yes_output_is_bounded_for_large_input() {
        let huge = "a".repeat(MAX_OUTPUT_BYTES * 2);
        let out = build_yes_output(&huge);
        assert!(out.len() <= MAX_OUTPUT_BYTES);
        assert_eq!(out.lines().count(), 1);
    }

    #[test]
    fn yes_output_stays_at_existing_line_limit_for_small_input() {
        let out = build_yes_output("y");
        assert_eq!(out.lines().count(), 10_000);
    }
}
