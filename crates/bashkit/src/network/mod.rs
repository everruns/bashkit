//! Network layer for BashKit
//!
//! Provides secure network access with URL allowlists.
//!
//! # Security Model
//!
//! - Network access is disabled by default
//! - URLs must match an entry in the allowlist
//! - Allowlist entries can match by scheme, host, and path prefix

mod allowlist;

#[cfg(feature = "network")]
mod client;

#[allow(unused_imports)] // UrlMatch is used internally but may not be exported
pub use allowlist::{NetworkAllowlist, UrlMatch};

#[cfg(feature = "network")]
pub use client::HttpClient;
