//! SSH support for Bashkit
//!
//! Provides SSH/SCP/SFTP operations via the `ssh` feature flag.
//! Follows the same opt-in pattern as `git` and `http_client`.
//!
//! # Security Model
//!
//! - **Disabled by default**: SSH requires explicit configuration
//! - **Host allowlist**: Only allowed hosts can be connected to (default-deny)
//! - **No credential leakage**: Keys read from VFS only, never host `~/.ssh/`
//! - **Resource limits**: Timeouts, max response size, max sessions
//!
//! # Usage
//!
//! ```rust,ignore
//! use bashkit::{Bash, SshConfig};
//!
//! let mut bash = Bash::builder()
//!     .ssh(SshConfig::new()
//!         .allow("*.supabase.co")
//!         .default_user("root"))
//!     .build();
//!
//! let result = bash.exec("ssh db.abc.supabase.co 'psql -c \"SELECT 1\"'").await?;
//! ```
//!
//! # Security Threats
//!
//! See `specs/ssh-support.md` and `specs/threat-model.md` (TM-SSH-*)

mod allowlist;
mod config;

#[cfg(feature = "ssh")]
mod client;

#[cfg(feature = "ssh")]
mod handler;

#[cfg(feature = "ssh")]
mod russh_handler;

pub use allowlist::SshAllowlist;
pub use config::{SshConfig, TrustedHostKey};

#[cfg(feature = "ssh")]
pub use client::SshClient;

#[cfg(feature = "ssh")]
pub use handler::{SshHandler, SshOutput, SshTarget};
