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
//! See `specs/006-threat-model.md` Section 9: Git Security (TM-GIT-*)

mod config;

#[cfg(feature = "git")]
mod client;

pub use config::GitConfig;

#[cfg(feature = "git")]
pub use client::GitClient;
