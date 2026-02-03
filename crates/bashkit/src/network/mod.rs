//! Network layer for BashKit
//!
//! Provides secure network access with URL allowlists.
//!
//! # Security Model
//!
//! - Network access is disabled by default
//! - URLs must match an entry in the allowlist
//! - Allowlist entries can match by scheme, host, and path prefix
//! - Response size is limited to prevent memory exhaustion
//! - Timeouts prevent hanging on unresponsive servers
//! - Redirects are not followed automatically (prevents allowlist bypass)

mod allowlist;

#[cfg(feature = "http_client")]
mod client;

#[allow(unused_imports)] // UrlMatch is used internally but may not be exported
pub use allowlist::{NetworkAllowlist, UrlMatch};

#[cfg(feature = "http_client")]
pub use client::{HttpClient, Method, Response};
