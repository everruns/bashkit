//! Git support for Bashkit
//!
//! Provides virtual git operations on the virtual filesystem.
//! Requires the `git` feature to be enabled.
//!
//! # Security Model
//!
//! - **Disabled by default**: Git access requires explicit configuration
//! - **Virtual filesystem only**: All operations confined to VFS
//! - **Remote URL allowlist**: Only allowed URLs can be accessed (Phase 2)
//! - **Virtual identity**: Author name/email are configurable, never from host
//! - **No host access**: Cannot read host ~/.gitconfig or credentials
//!
//! # Usage
//!
//! Configure git access using [`GitConfig`] with [`crate::Bash::builder`]:
//!
//! ```rust,ignore
//! use bashkit::{Bash, GitConfig};
//!
//! # #[tokio::main]
//! # async fn main() -> bashkit::Result<()> {
//! let mut bash = Bash::builder()
//!     .git(GitConfig::new()
//!         .author("Bot", "bot@example.com"))
//!     .build();
//!
//! // Now git commands work on the virtual filesystem
//! let result = bash.exec("git init /repo && cd /repo && git status").await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Supported Commands (Phase 1)
//!
//! - `git init [path]` - Create empty repository
//! - `git config [key] [value]` - Get/set config
//! - `git add <pathspec>...` - Stage files
//! - `git commit -m <message>` - Record changes
//! - `git status` - Show working tree status
//! - `git log [-n N]` - Show commit history
//!
//! # Security Threats
//!
//! See `specs/threat-model.md` Section 9: Git Security (TM-GIT-*)

mod config;

#[cfg(feature = "git")]
mod client;

#[cfg(feature = "git")]
mod cmd;

pub use config::GitConfig;

#[cfg(feature = "git")]
pub use client::GitClient;

#[cfg(feature = "git")]
pub use cmd::Git;

/// THREAT[TM-GIT-015]: Sanitize git output to prevent terminal injection.
///
/// Strips ANSI escape sequences, null bytes, and dangerous C0/C1 control
/// characters from strings before they reach interpreter stdout. Preserves
/// tab (0x09), newline (0x0a), and carriage return (0x0d).
#[cfg(any(feature = "git", test))]
pub(crate) fn sanitize_git_output(s: &str) -> String {
    use regex::Regex;
    use std::sync::LazyLock;

    // Match ANSI escape sequences: ESC [ <params> <final byte>
    // Also matches OSC sequences: ESC ] ... BEL
    // Also matches two-char ESC sequences (e.g. ESC H)
    static ANSI_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]|\x1b\][^\x07]*\x07|\x1b[^\[\]].?")
            .expect("ANSI regex is valid")
    });

    let stripped = ANSI_RE.replace_all(s, "");

    stripped
        .chars()
        .filter(|&ch| {
            // Allow printable ASCII + common whitespace
            if ch == '\t' || ch == '\n' || ch == '\r' {
                return true;
            }
            // Remove C0 control chars (0x00-0x1f) except the three above
            if (ch as u32) <= 0x1f {
                return false;
            }
            // Remove DEL
            if ch as u32 == 0x7f {
                return false;
            }
            // Remove C1 control chars (0x80-0x9f)
            if (0x80..=0x9f).contains(&(ch as u32)) {
                return false;
            }
            true
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::sanitize_git_output;

    #[test]
    fn test_strips_ansi_escape_sequences() {
        let input = "main\x1b[31mPWNED\x1b[0m";
        assert_eq!(sanitize_git_output(input), "mainPWNED");
    }

    #[test]
    fn test_strips_null_bytes() {
        let input = "hello\x00world";
        assert_eq!(sanitize_git_output(input), "helloworld");
    }

    #[test]
    fn test_strips_c0_control_chars() {
        // 0x01 (SOH), 0x02 (STX), 0x07 (BEL), 0x08 (BS)
        let input = "a\x01b\x02c\x07d\x08e";
        assert_eq!(sanitize_git_output(input), "abcde");
    }

    #[test]
    fn test_preserves_tab_newline_cr() {
        let input = "line1\n\tindented\r\nline2";
        assert_eq!(sanitize_git_output(input), "line1\n\tindented\r\nline2");
    }

    #[test]
    fn test_strips_c1_control_chars() {
        // 0x80, 0x9b (CSI), 0x9f
        let input = format!("a{}b{}c{}d", '\u{0080}', '\u{009b}', '\u{009f}');
        assert_eq!(sanitize_git_output(&input), "abcd");
    }

    #[test]
    fn test_strips_del() {
        let input = "hello\x7fworld";
        assert_eq!(sanitize_git_output(input), "helloworld");
    }

    #[test]
    fn test_complex_ansi_injection() {
        // Simulates a crafted branch name with multiple escape sequences
        let input = "\x1b[2J\x1b[Hfake-prompt$ rm -rf /\x1b[0m";
        assert_eq!(sanitize_git_output(input), "fake-prompt$ rm -rf /");
    }

    #[test]
    fn test_passthrough_normal_text() {
        let input = "feature/my-branch-123";
        assert_eq!(sanitize_git_output(input), "feature/my-branch-123");
    }

    #[test]
    fn test_unicode_preserved() {
        let input = "日本語ブランチ";
        assert_eq!(sanitize_git_output(input), "日本語ブランチ");
    }
}
