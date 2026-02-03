//! Network layer for BashKit
//!
//! Provides secure HTTP access with URL allowlists for `curl` and `wget` builtins.
//!
//! This module requires the `http_client` feature to be enabled.
//!
//! # Security Model
//!
//! - **Disabled by default**: Network access requires explicit allowlist configuration
//! - **URL allowlist**: Only URLs matching configured patterns are permitted
//! - **Scheme enforcement**: HTTPS/HTTP schemes are validated
//! - **Response size limits**: Default 10MB limit prevents memory exhaustion
//! - **Timeouts**: 30 second default prevents hanging on slow servers
//! - **No automatic redirects**: Prevents allowlist bypass via redirect chains
//! - **Zip bomb protection**: Compressed responses are size-limited during decompression
//!
//! # Usage
//!
//! Configure network access using [`NetworkAllowlist`] with [`crate::Bash::builder`]:
//!
//! ```rust,no_run
//! use bashkit::{Bash, NetworkAllowlist};
//!
//! # #[tokio::main]
//! # async fn main() -> bashkit::Result<()> {
//! let mut bash = Bash::builder()
//!     .network(NetworkAllowlist::new()
//!         .allow("https://api.example.com")
//!         .allow("https://cdn.example.com/assets/"))
//!     .build();
//!
//! // Now curl/wget can access allowed URLs
//! let result = bash.exec("curl -s https://api.example.com/data").await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Allowlist Patterns
//!
//! The allowlist supports several matching modes:
//!
//! - **Exact host**: `https://api.example.com` - matches only this host
//! - **Path prefix**: `https://api.example.com/v1/` - matches URLs under this path
//! - **Port specific**: `https://api.example.com:8443` - matches specific port
//! - **Allow all** (use with caution): [`NetworkAllowlist::allow_all()`]
//!
//! # curl/wget Builtins
//!
//! When the `http_client` feature is enabled and an allowlist is configured,
//! the following builtins become functional:
//!
//! ## curl
//!
//! ```bash
//! # GET request
//! curl -s https://api.example.com/data
//!
//! # POST with data
//! curl -X POST -d '{"key":"value"}' -H "Content-Type: application/json" https://api.example.com
//!
//! # Save to file
//! curl -o /tmp/data.json https://api.example.com/data
//!
//! # With authentication
//! curl -u user:pass https://api.example.com/private
//!
//! # Request compressed response
//! curl --compressed https://api.example.com/large-data
//! ```
//!
//! ## wget
//!
//! ```bash
//! # Download file
//! wget -O /tmp/file.txt https://cdn.example.com/file.txt
//!
//! # Check if URL exists
//! wget --spider https://cdn.example.com/file.txt
//!
//! # POST request
//! wget --post-data='key=value' -O - https://api.example.com/submit
//! ```

mod allowlist;

#[cfg(feature = "http_client")]
mod client;

#[allow(unused_imports)] // UrlMatch is used internally but may not be exported
pub use allowlist::{NetworkAllowlist, UrlMatch};

#[cfg(feature = "http_client")]
pub use client::{HttpClient, Method, Response};
