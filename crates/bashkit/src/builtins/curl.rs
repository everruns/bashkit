//! Curl and wget builtins - transfer data from URLs
//!
//! Note: These builtins require the `http_client` feature and proper configuration.
//! Network access is restricted by allowlist for security.
//!
//! # Security
//!
//! - URLs must match the configured allowlist
//! - Response size is limited (default: 10MB) to prevent memory exhaustion
//! - Timeouts prevent hanging on unresponsive servers
//! - Redirects are not followed automatically (to prevent allowlist bypass)
//! - Compression decompression is size-limited to prevent zip bombs

use async_trait::async_trait;

#[cfg(feature = "http_client")]
use super::resolve_path;
use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The curl builtin - transfer data from URLs.
///
/// Usage: curl [OPTIONS] URL
///
/// Options:
///   -s, --silent       Silent mode (no progress)
///   -o FILE            Write output to FILE
///   -X METHOD          Specify request method (GET, POST, PUT, DELETE, HEAD)
///   -d DATA            Send data in request body (implies POST if no -X)
///   -H HEADER          Add header to request (format: "Name: Value")
///   -I, --head         Fetch headers only (HEAD request)
///   -f, --fail         Fail silently on HTTP errors (no output)
///   -L, --location     Follow redirects (up to 10 redirects)
///   -w FORMAT          Write output format after transfer
///   --compressed       Request compressed response and decompress
///   -u, --user U:P     Basic authentication (user:password)
///   -A, --user-agent S Custom user agent string
///   -e, --referer URL  Referer URL
///   -m, --max-time S   Maximum time in seconds for operation
///   --connect-timeout S Maximum time in seconds for connection
///   -v, --verbose      Verbose output
///
/// Note: Network access requires the 'http_client' feature and proper
/// URL allowlist configuration. Without configuration, all requests
/// will fail with an access denied error.
///
/// # Security
///
/// - Response size is limited to prevent memory exhaustion (applies to decompressed size too)
/// - Redirects require each URL to be in the allowlist
/// - Timeouts prevent hanging on slow servers
/// - --compressed decompression is size-limited to prevent zip bombs
pub struct Curl;

#[async_trait]
impl Builtin for Curl {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Parse arguments
        let mut silent = false;
        let mut verbose = false;
        let mut output_file: Option<String> = None;
        let mut method = "GET".to_string();
        let mut data: Option<String> = None;
        let mut headers: Vec<String> = Vec::new();
        let mut head_only = false;
        let mut fail_on_error = false;
        let mut follow_redirects = false;
        let mut write_out: Option<String> = None;
        let mut compressed = false;
        let mut user_auth: Option<String> = None;
        let mut user_agent: Option<String> = None;
        let mut referer: Option<String> = None;
        let mut max_time: Option<u64> = None;
        let mut connect_timeout: Option<u64> = None;
        let mut url: Option<String> = None;
        let mut form_fields: Vec<String> = Vec::new();

        let mut i = 0;
        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            match arg.as_str() {
                "-s" | "--silent" => silent = true,
                "-v" | "--verbose" => verbose = true,
                "-f" | "--fail" => fail_on_error = true,
                "-L" | "--location" => follow_redirects = true,
                "--compressed" => compressed = true,
                "-I" | "--head" => {
                    head_only = true;
                    method = "HEAD".to_string();
                }
                "-o" => {
                    i += 1;
                    if i < ctx.args.len() {
                        output_file = Some(ctx.args[i].clone());
                    }
                }
                "-X" => {
                    i += 1;
                    if i < ctx.args.len() {
                        method = ctx.args[i].clone().to_uppercase();
                    }
                }
                "-d" | "--data" => {
                    i += 1;
                    if i < ctx.args.len() {
                        data = Some(ctx.args[i].clone());
                        if method == "GET" {
                            method = "POST".to_string();
                        }
                    }
                }
                "-H" | "--header" => {
                    i += 1;
                    if i < ctx.args.len() {
                        headers.push(ctx.args[i].clone());
                    }
                }
                "-w" | "--write-out" => {
                    i += 1;
                    if i < ctx.args.len() {
                        write_out = Some(ctx.args[i].clone());
                    }
                }
                "-u" | "--user" => {
                    i += 1;
                    if i < ctx.args.len() {
                        user_auth = Some(ctx.args[i].clone());
                    }
                }
                "-A" | "--user-agent" => {
                    i += 1;
                    if i < ctx.args.len() {
                        user_agent = Some(ctx.args[i].clone());
                    }
                }
                "-e" | "--referer" => {
                    i += 1;
                    if i < ctx.args.len() {
                        referer = Some(ctx.args[i].clone());
                    }
                }
                "-m" | "--max-time" => {
                    i += 1;
                    if i < ctx.args.len() {
                        max_time = ctx.args[i].parse().ok();
                    }
                }
                "--connect-timeout" => {
                    i += 1;
                    if i < ctx.args.len() {
                        connect_timeout = ctx.args[i].parse().ok();
                    }
                }
                "-F" | "--form" => {
                    i += 1;
                    if i < ctx.args.len() {
                        form_fields.push(ctx.args[i].clone());
                        if method == "GET" {
                            method = "POST".to_string();
                        }
                    }
                }
                _ if !arg.starts_with('-') => {
                    url = Some(arg.clone());
                }
                _ => {
                    // Ignore unknown options for compatibility
                }
            }
            i += 1;
        }

        // Validate URL
        let url = match url {
            Some(u) => u,
            None => {
                return Ok(ExecResult::err("curl: no URL specified\n".to_string(), 3));
            }
        };

        // Check if network is configured
        #[cfg(feature = "http_client")]
        {
            if let Some(http_client) = ctx.http_client {
                return execute_curl_request(
                    http_client,
                    &url,
                    &method,
                    data.as_deref(),
                    &headers,
                    head_only,
                    silent,
                    verbose,
                    fail_on_error,
                    follow_redirects,
                    write_out.as_deref(),
                    output_file.as_deref(),
                    compressed,
                    user_auth.as_deref(),
                    user_agent.as_deref(),
                    referer.as_deref(),
                    max_time,
                    connect_timeout,
                    &form_fields,
                    &ctx,
                )
                .await;
            }
        }

        // Network not configured
        let _ = (
            silent,
            verbose,
            output_file,
            method,
            data,
            headers,
            head_only,
            fail_on_error,
            follow_redirects,
            write_out,
            compressed,
            user_auth,
            user_agent,
            referer,
            max_time,
            connect_timeout,
            form_fields,
        );

        Ok(ExecResult::err(
            format!(
                "curl: network access not configured\nURL: {}\n\
                 Note: Network builtins require the 'http_client' feature and\n\
                 URL allowlist configuration for security.\n",
                url
            ),
            1,
        ))
    }
}

/// Execute the actual curl request when http_client feature is enabled.
#[cfg(feature = "http_client")]
#[allow(clippy::too_many_arguments)]
async fn execute_curl_request(
    http_client: &crate::network::HttpClient,
    url: &str,
    method: &str,
    data: Option<&str>,
    headers: &[String],
    head_only: bool,
    _silent: bool,
    verbose: bool,
    fail_on_error: bool,
    follow_redirects: bool,
    write_out: Option<&str>,
    output_file: Option<&str>,
    compressed: bool,
    user_auth: Option<&str>,
    user_agent: Option<&str>,
    referer: Option<&str>,
    max_time: Option<u64>,
    connect_timeout: Option<u64>,
    form_fields: &[String],
    ctx: &Context<'_>,
) -> Result<ExecResult> {
    use crate::network::Method;

    // Parse method
    let http_method = match method {
        "GET" => Method::Get,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "DELETE" => Method::Delete,
        "HEAD" => Method::Head,
        "PATCH" => Method::Patch,
        _ => {
            return Ok(ExecResult::err(
                format!("curl: unsupported method: {}\n", method),
                1,
            ));
        }
    };

    // Parse headers and add custom ones
    let mut header_pairs: Vec<(String, String)> = Vec::new();
    for header in headers {
        if let Some(colon_pos) = header.find(':') {
            let name = header[..colon_pos].trim().to_string();
            let value = header[colon_pos + 1..].trim().to_string();
            header_pairs.push((name, value));
        }
    }

    // Add --compressed header (request gzip/deflate)
    if compressed {
        header_pairs.push(("Accept-Encoding".to_string(), "gzip, deflate".to_string()));
    }

    // Add basic auth header
    if let Some(auth) = user_auth {
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(auth);
        header_pairs.push(("Authorization".to_string(), format!("Basic {}", encoded)));
    }

    // Add custom user agent
    if let Some(ua) = user_agent {
        header_pairs.push(("User-Agent".to_string(), ua.to_string()));
    }

    // Add referer
    if let Some(ref_url) = referer {
        header_pairs.push(("Referer".to_string(), ref_url.to_string()));
    }

    // Verbose output buffer
    let mut verbose_output = String::new();

    // Build multipart body if -F fields are present
    let multipart_body: Option<Vec<u8>> = if !form_fields.is_empty() {
        let boundary = format!(
            "----bashkit{:016x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        header_pairs.push((
            "Content-Type".to_string(),
            format!("multipart/form-data; boundary={}", boundary),
        ));
        let mut body = Vec::new();
        for field in form_fields {
            if let Some(eq_pos) = field.find('=') {
                let name = &field[..eq_pos];
                let value = &field[eq_pos + 1..];
                body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
                if let Some(file_path) = value.strip_prefix('@') {
                    // File upload: key=@filepath[;type=mime]
                    let (path, mime) = if let Some(semi) = file_path.find(';') {
                        let p = &file_path[..semi];
                        let rest = &file_path[semi + 1..];
                        let m = rest
                            .strip_prefix("type=")
                            .unwrap_or("application/octet-stream");
                        (p, m.to_string())
                    } else {
                        (file_path, guess_mime(file_path))
                    };
                    let resolved = resolve_path(ctx.cwd, path);
                    let file_content = ctx.fs.read_file(&resolved).await.unwrap_or_default();
                    let filename = std::path::Path::new(path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "file".to_string());
                    body.extend_from_slice(
                        format!(
                            "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n",
                            name, filename
                        )
                        .as_bytes(),
                    );
                    body.extend_from_slice(format!("Content-Type: {}\r\n\r\n", mime).as_bytes());
                    body.extend_from_slice(&file_content);
                } else {
                    // Text field: key=value
                    body.extend_from_slice(
                        format!("Content-Disposition: form-data; name=\"{}\"\r\n\r\n", name)
                            .as_bytes(),
                    );
                    body.extend_from_slice(value.as_bytes());
                }
                body.extend_from_slice(b"\r\n");
            }
        }
        body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());
        Some(body)
    } else {
        None
    };

    // Make the request
    let body = if multipart_body.is_some() {
        multipart_body.as_deref()
    } else {
        data.map(|d| d.as_bytes())
    };
    let mut current_url = url.to_string();
    let mut redirect_count = 0;
    const MAX_REDIRECTS: u32 = 10;

    loop {
        if verbose {
            verbose_output.push_str(&format!("> {} {} HTTP/1.1\r\n", method, current_url));
            for (name, value) in &header_pairs {
                verbose_output.push_str(&format!("> {}: {}\r\n", name, value));
            }
            verbose_output.push_str(">\r\n");
        }

        let result = http_client
            .request_with_timeouts(
                http_method,
                &current_url,
                body,
                &header_pairs,
                max_time,
                connect_timeout,
            )
            .await;

        match result {
            Ok(response) => {
                if verbose {
                    verbose_output.push_str(&format!("< HTTP/1.1 {}\r\n", response.status));
                    for (name, value) in &response.headers {
                        verbose_output.push_str(&format!("< {}: {}\r\n", name, value));
                    }
                    verbose_output.push_str("<\r\n");
                }

                // Handle redirects if -L flag is set
                if follow_redirects
                    && (response.status == 301
                        || response.status == 302
                        || response.status == 303
                        || response.status == 307
                        || response.status == 308)
                {
                    redirect_count += 1;
                    if redirect_count > MAX_REDIRECTS {
                        return Ok(ExecResult::err(
                            format!("curl: maximum redirects ({}) exceeded\n", MAX_REDIRECTS),
                            47,
                        ));
                    }

                    // Find Location header
                    if let Some((_, location)) = response
                        .headers
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case("location"))
                    {
                        current_url = resolve_redirect_url(&current_url, location);
                        continue;
                    }
                }

                // Check for HTTP errors if -f flag is set
                if fail_on_error && response.status >= 400 {
                    return Ok(ExecResult {
                        stdout: String::new(),
                        stderr: format!(
                            "curl: (22) The requested URL returned error: {}\n",
                            response.status
                        ),
                        exit_code: 22,
                        control_flow: crate::interpreter::ControlFlow::None,
                    });
                }

                // Get response body, potentially decompressing
                let body_bytes = if compressed {
                    // Check Content-Encoding header
                    let encoding = response
                        .headers
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case("content-encoding"))
                        .map(|(_, v)| v.as_str());

                    match encoding {
                        Some("gzip") => {
                            decompress_gzip(&response.body, http_client.max_response_bytes())?
                        }
                        Some("deflate") => {
                            decompress_deflate(&response.body, http_client.max_response_bytes())?
                        }
                        _ => response.body.clone(),
                    }
                } else {
                    response.body.clone()
                };

                // Format output
                let output = if head_only {
                    // For -I, output headers
                    let mut header_output = format!("HTTP/1.1 {} OK\r\n", response.status);
                    for (name, value) in &response.headers {
                        header_output.push_str(&format!("{}: {}\r\n", name, value));
                    }
                    header_output.push_str("\r\n");
                    header_output
                } else {
                    String::from_utf8_lossy(&body_bytes).into_owned()
                };

                // Write to file if -o specified
                if let Some(file_path) = output_file {
                    let full_path = resolve_path(ctx.cwd, file_path);
                    if let Err(e) = ctx.fs.write_file(&full_path, output.as_bytes()).await {
                        return Ok(ExecResult::err(
                            format!("curl: failed to write to {}: {}\n", file_path, e),
                            23,
                        ));
                    }
                    // Output write-out format if specified
                    let mut stdout = verbose_output;
                    if let Some(fmt) = write_out {
                        stdout.push_str(&format_write_out(fmt, &response, output.len()));
                    }
                    return Ok(ExecResult::ok(stdout));
                }

                // Append write-out format if specified
                let mut final_output = verbose_output;
                final_output.push_str(&output);
                if let Some(fmt) = write_out {
                    final_output.push_str(&format_write_out(fmt, &response, output.len()));
                }

                return Ok(ExecResult::ok(final_output));
            }
            Err(e) => {
                let error_msg = e.to_string();

                // Determine appropriate exit code based on error type
                let exit_code = if error_msg.contains("access denied") {
                    7 // curl: couldn't connect to host
                } else if error_msg.contains("timeout") || error_msg.contains("timed out") {
                    28 // curl: operation timed out
                } else if error_msg.contains("response too large") {
                    63 // curl: maximum file size exceeded
                } else if error_msg.contains("invalid URL") {
                    3 // curl: URL malformed
                } else {
                    1 // general error
                };

                return Ok(ExecResult::err(format!("curl: {}\n", error_msg), exit_code));
            }
        }
    }
}

/// Guess MIME type from file extension
#[cfg(feature = "http_client")]
fn guess_mime(path: &str) -> String {
    match std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
    {
        Some("json") => "application/json",
        Some("xml") => "application/xml",
        Some("html" | "htm") => "text/html",
        Some("txt" | "log" | "csv") => "text/plain",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("pdf") => "application/pdf",
        Some("gz" | "tgz") => "application/gzip",
        Some("tar") => "application/x-tar",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// Resolve a redirect URL which may be relative.
fn resolve_redirect_url(base: &str, location: &str) -> String {
    if location.starts_with("http://") || location.starts_with("https://") {
        location.to_string()
    } else if location.starts_with('/') {
        // Absolute path - combine with base scheme and host
        if let Ok(base_url) = url::Url::parse(base) {
            format!(
                "{}://{}{}",
                base_url.scheme(),
                base_url.host_str().unwrap_or(""),
                location
            )
        } else {
            location.to_string()
        }
    } else {
        // Relative path
        if let Ok(base_url) = url::Url::parse(base) {
            if let Ok(resolved) = base_url.join(location) {
                return resolved.to_string();
            }
        }
        location.to_string()
    }
}

/// Format the -w/--write-out output.
#[cfg(feature = "http_client")]
fn format_write_out(fmt: &str, response: &crate::network::Response, size: usize) -> String {
    let mut output = fmt.to_string();
    output = output.replace("%{http_code}", &response.status.to_string());
    output = output.replace("%{size_download}", &size.to_string());
    output = output.replace("%{content_type}", {
        response
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.as_str())
            .unwrap_or("")
    });
    output = output.replace("\\n", "\n");
    output = output.replace("\\t", "\t");
    output
}

/// Decompress gzip data with size limit.
///
/// Returns error if decompressed size exceeds max_size (prevents zip bombs).
#[cfg(feature = "http_client")]
fn decompress_gzip(data: &[u8], max_size: usize) -> Result<Vec<u8>> {
    use flate2::read::GzDecoder;
    use std::io::Read;

    let mut decoder = GzDecoder::new(data);
    let mut decompressed = Vec::new();
    let mut buffer = [0u8; 8192];

    loop {
        match decoder.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                if decompressed.len() + n > max_size {
                    return Err(crate::error::Error::Network(format!(
                        "decompressed response too large: exceeded {} bytes limit",
                        max_size
                    )));
                }
                decompressed.extend_from_slice(&buffer[..n]);
            }
            Err(e) => {
                return Err(crate::error::Error::Network(format!(
                    "gzip decompression failed: {}",
                    e
                )));
            }
        }
    }

    Ok(decompressed)
}

/// Decompress deflate data with size limit.
///
/// Returns error if decompressed size exceeds max_size (prevents zip bombs).
#[cfg(feature = "http_client")]
fn decompress_deflate(data: &[u8], max_size: usize) -> Result<Vec<u8>> {
    use flate2::read::DeflateDecoder;
    use std::io::Read;

    let mut decoder = DeflateDecoder::new(data);
    let mut decompressed = Vec::new();
    let mut buffer = [0u8; 8192];

    loop {
        match decoder.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                if decompressed.len() + n > max_size {
                    return Err(crate::error::Error::Network(format!(
                        "decompressed response too large: exceeded {} bytes limit",
                        max_size
                    )));
                }
                decompressed.extend_from_slice(&buffer[..n]);
            }
            Err(e) => {
                return Err(crate::error::Error::Network(format!(
                    "deflate decompression failed: {}",
                    e
                )));
            }
        }
    }

    Ok(decompressed)
}

/// The wget builtin - download files from URLs.
///
/// Usage: wget [OPTIONS] URL
///
/// Options:
///   -q, --quiet        Quiet mode (no progress output)
///   -O FILE            Write output to FILE (use '-' for stdout)
///   --spider           Don't download, just check if URL exists
///   --header "H: V"    Add custom header
///   -U, --user-agent S Custom user agent string
///   --post-data DATA   POST data with request
///   -t, --tries N      Number of retries (ignored, for compatibility)
///   -T, --timeout S    Timeout in seconds for all operations
///   --connect-timeout S Timeout in seconds for connection
///
/// Note: Network access requires the 'http_client' feature and proper
/// URL allowlist configuration.
///
/// # Security
///
/// - Response size is limited to prevent memory exhaustion
/// - Only URLs in the allowlist can be accessed
pub struct Wget;

#[async_trait]
impl Builtin for Wget {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Parse arguments
        let mut quiet = false;
        let mut output_file: Option<String> = None;
        let mut spider = false;
        let mut headers: Vec<String> = Vec::new();
        let mut user_agent: Option<String> = None;
        let mut post_data: Option<String> = None;
        let mut timeout: Option<u64> = None;
        let mut connect_timeout: Option<u64> = None;
        let mut url: Option<String> = None;

        let mut i = 0;
        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            match arg.as_str() {
                "-q" | "--quiet" => quiet = true,
                "--spider" => spider = true,
                "-O" => {
                    i += 1;
                    if i < ctx.args.len() {
                        output_file = Some(ctx.args[i].clone());
                    }
                }
                "--header" => {
                    i += 1;
                    if i < ctx.args.len() {
                        headers.push(ctx.args[i].clone());
                    }
                }
                "-U" | "--user-agent" => {
                    i += 1;
                    if i < ctx.args.len() {
                        user_agent = Some(ctx.args[i].clone());
                    }
                }
                "--post-data" => {
                    i += 1;
                    if i < ctx.args.len() {
                        post_data = Some(ctx.args[i].clone());
                    }
                }
                "-t" | "--tries" => {
                    // Ignore retry count (for compatibility)
                    i += 1;
                }
                "-T" | "--timeout" => {
                    i += 1;
                    if i < ctx.args.len() {
                        timeout = ctx.args[i].parse().ok();
                    }
                }
                "--connect-timeout" => {
                    i += 1;
                    if i < ctx.args.len() {
                        connect_timeout = ctx.args[i].parse().ok();
                    }
                }
                _ if !arg.starts_with('-') => {
                    url = Some(arg.clone());
                }
                _ => {
                    // Ignore unknown options
                }
            }
            i += 1;
        }

        // Validate URL
        let url = match url {
            Some(u) => u,
            None => {
                return Ok(ExecResult::err("wget: missing URL\n".to_string(), 1));
            }
        };

        // Check if network is configured
        #[cfg(feature = "http_client")]
        {
            if let Some(http_client) = ctx.http_client {
                return execute_wget_request(
                    http_client,
                    &url,
                    quiet,
                    spider,
                    output_file.as_deref(),
                    &headers,
                    user_agent.as_deref(),
                    post_data.as_deref(),
                    timeout,
                    connect_timeout,
                    &ctx,
                )
                .await;
            }
        }

        // Network not configured
        let _ = (
            quiet,
            output_file,
            spider,
            headers,
            user_agent,
            post_data,
            timeout,
            connect_timeout,
        );

        Ok(ExecResult::err(
            format!(
                "wget: network access not configured\nURL: {}\n\
                 Note: Network builtins require the 'http_client' feature and\n\
                 URL allowlist configuration for security.\n",
                url
            ),
            1,
        ))
    }
}

/// Execute the actual wget request when http_client feature is enabled.
#[cfg(feature = "http_client")]
#[allow(clippy::too_many_arguments)]
async fn execute_wget_request(
    http_client: &crate::network::HttpClient,
    url: &str,
    quiet: bool,
    spider: bool,
    output_file: Option<&str>,
    headers: &[String],
    user_agent: Option<&str>,
    post_data: Option<&str>,
    timeout: Option<u64>,
    connect_timeout: Option<u64>,
    ctx: &Context<'_>,
) -> Result<ExecResult> {
    use crate::network::Method;

    // Build header pairs
    let mut header_pairs: Vec<(String, String)> = Vec::new();
    for header in headers {
        if let Some(colon_pos) = header.find(':') {
            let name = header[..colon_pos].trim().to_string();
            let value = header[colon_pos + 1..].trim().to_string();
            header_pairs.push((name, value));
        }
    }

    // Add custom user agent
    if let Some(ua) = user_agent {
        header_pairs.push(("User-Agent".to_string(), ua.to_string()));
    }

    // Determine method and body
    let (method, body) = if spider {
        (Method::Head, None)
    } else if post_data.is_some() {
        (Method::Post, post_data.map(|d| d.as_bytes()))
    } else {
        (Method::Get, None)
    };

    let result = http_client
        .request_with_timeouts(method, url, body, &header_pairs, timeout, connect_timeout)
        .await;

    match result {
        Ok(response) => {
            // Spider mode - just check if accessible
            if spider {
                if response.status >= 200 && response.status < 400 {
                    let msg = if quiet {
                        String::new()
                    } else {
                        format!("Spider mode enabled. Check if remote file exists.\nHTTP request sent, awaiting response... {} OK\nRemote file exists.\n", response.status)
                    };
                    return Ok(ExecResult::ok(msg));
                } else {
                    return Ok(ExecResult::err(
                        format!(
                            "Remote file does not exist -- broken link!!!\n\
                             HTTP request sent, awaiting response... {} Error\n",
                            response.status
                        ),
                        8,
                    ));
                }
            }

            // Determine output filename
            let output_path = if let Some(file) = output_file {
                if file == "-" {
                    // Output to stdout
                    return Ok(ExecResult::ok(response.body_string()));
                }
                file.to_string()
            } else {
                // Extract filename from URL
                extract_filename_from_url(url)
            };

            // Progress output
            let mut stderr_msg = String::new();
            if !quiet {
                stderr_msg.push_str(&format!(
                    "Connecting to {}... connected.\n\
                     HTTP request sent, awaiting response... {} OK\n\
                     Length: {} [{}]\n\
                     Saving to: '{}'\n\n",
                    extract_host_from_url(url),
                    response.status,
                    response.body.len(),
                    response
                        .headers
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                        .map(|(_, v)| v.as_str())
                        .unwrap_or("application/octet-stream"),
                    output_path
                ));
            }

            // Write to file
            let full_path = resolve_path(ctx.cwd, &output_path);
            if let Err(e) = ctx.fs.write_file(&full_path, &response.body).await {
                return Ok(ExecResult::err(
                    format!("wget: failed to write to {}: {}\n", output_path, e),
                    1,
                ));
            }

            if !quiet {
                stderr_msg.push_str(&format!(
                    "'{}' saved [{}/{}]\n",
                    output_path,
                    response.body.len(),
                    response.body.len()
                ));
            }

            Ok(ExecResult {
                stdout: String::new(),
                stderr: stderr_msg,
                exit_code: 0,
                control_flow: crate::interpreter::ControlFlow::None,
            })
        }
        Err(e) => {
            let error_msg = e.to_string();

            // Determine appropriate exit code
            let exit_code = if error_msg.contains("access denied") || error_msg.contains("timeout")
            {
                4 // Network failure
            } else {
                1 // General error
            };

            Ok(ExecResult::err(format!("wget: {}\n", error_msg), exit_code))
        }
    }
}

/// Extract filename from URL for wget default output.
#[cfg(feature = "http_client")]
fn extract_filename_from_url(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        let path = parsed.path();
        if let Some(filename) = path.rsplit('/').next() {
            if !filename.is_empty() {
                return filename.to_string();
            }
        }
    }
    "index.html".to_string()
}

/// Extract host from URL for wget progress output.
#[cfg(feature = "http_client")]
fn extract_host_from_url(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        if let Some(host) = parsed.host_str() {
            return host.to_string();
        }
    }
    "unknown".to_string()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_curl(args: &[&str]) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");

        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        Curl.execute(ctx).await.unwrap()
    }

    async fn run_wget(args: &[&str]) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");

        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        Wget.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_curl_no_url() {
        let result = run_curl(&[]).await;
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("no URL specified"));
    }

    #[tokio::test]
    async fn test_curl_with_url_no_network() {
        let result = run_curl(&["https://example.com"]).await;
        // Should fail gracefully without network config
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("network access not configured"));
    }

    #[tokio::test]
    async fn test_wget_no_url() {
        let result = run_wget(&[]).await;
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("missing URL"));
    }

    #[tokio::test]
    async fn test_wget_with_url_no_network() {
        let result = run_wget(&["https://example.com"]).await;
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("network access not configured"));
    }

    #[cfg(feature = "http_client")]
    mod network_tests {
        use super::*;

        #[test]
        fn test_extract_filename_from_url() {
            assert_eq!(
                extract_filename_from_url("https://example.com/file.txt"),
                "file.txt"
            );
            assert_eq!(
                extract_filename_from_url("https://example.com/path/to/document.pdf"),
                "document.pdf"
            );
            assert_eq!(
                extract_filename_from_url("https://example.com/"),
                "index.html"
            );
            assert_eq!(
                extract_filename_from_url("https://example.com"),
                "index.html"
            );
        }

        #[test]
        fn test_resolve_redirect_url_absolute() {
            let base = "https://example.com/original";
            assert_eq!(
                resolve_redirect_url(base, "https://other.com/new"),
                "https://other.com/new"
            );
        }

        #[test]
        fn test_resolve_redirect_url_absolute_path() {
            let base = "https://example.com/original/path";
            assert_eq!(
                resolve_redirect_url(base, "/new/path"),
                "https://example.com/new/path"
            );
        }

        #[test]
        fn test_resolve_redirect_url_relative() {
            let base = "https://example.com/original/";
            assert_eq!(
                resolve_redirect_url(base, "relative"),
                "https://example.com/original/relative"
            );
        }
    }
}
