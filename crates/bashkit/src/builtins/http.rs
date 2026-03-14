//! http builtin - HTTPie-inspired HTTP client (virtual stub)
//!
//! Parses HTTPie-style syntax and reports the request that would be sent.
//! If the `http_client` feature is enabled and configured, performs actual requests.

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// HTTP builtin - HTTPie-inspired HTTP client.
///
/// Usage: http [OPTIONS] [METHOD] URL [ITEMS...]
///
/// Items:
///   key=value      JSON body field (string)
///   key:=value     JSON body field (raw/number/bool)
///   Header:value   HTTP header
///   key==value     Query string parameter
///
/// Options:
///   --json, -j     Force JSON content type (default for data items)
///   --form, -f     Use form encoding instead of JSON
///   -v, --verbose  Show request and response headers
///   -h, --headers  Show response headers only
///   -b, --body     Show response body only (default)
///   -o FILE        Download to file
///
/// If METHOD is omitted, GET is used when no data items are present,
/// POST when data items are present.
///
/// In virtual environments without network, outputs the parsed request.
pub struct Http;

#[derive(Debug, PartialEq)]
enum ItemType {
    /// key=value -> JSON string field
    JsonField(String, String),
    /// key:=value -> JSON raw field (number, bool, null)
    JsonRawField(String, String),
    /// Header:value -> HTTP header
    Header(String, String),
    /// key==value -> query string parameter
    QueryParam(String, String),
}

struct HttpConfig {
    method: String,
    url: String,
    items: Vec<ItemType>,
    #[allow(dead_code)]
    json_mode: bool,
    form_mode: bool,
    verbose: bool,
    headers_only: bool,
    output_file: Option<String>,
}

fn is_http_method(s: &str) -> bool {
    matches!(
        s.to_uppercase().as_str(),
        "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS"
    )
}

fn parse_item(s: &str) -> Option<ItemType> {
    // Order matters: check `:=` and `==` before `=` and `:`
    if let Some(pos) = s.find(":=") {
        let key = &s[..pos];
        let val = &s[pos + 2..];
        if !key.is_empty() {
            return Some(ItemType::JsonRawField(key.to_string(), val.to_string()));
        }
    }
    if let Some(pos) = s.find("==") {
        let key = &s[..pos];
        let val = &s[pos + 2..];
        if !key.is_empty() {
            return Some(ItemType::QueryParam(key.to_string(), val.to_string()));
        }
    }
    if let Some(pos) = s.find('=') {
        // Make sure it's not := or ==
        if pos > 0 && &s[pos - 1..pos] != ":" && (pos + 1 >= s.len() || &s[pos + 1..pos + 2] != "=")
        {
            let key = &s[..pos];
            let val = &s[pos + 1..];
            return Some(ItemType::JsonField(key.to_string(), val.to_string()));
        }
    }
    if let Some(pos) = s.find(':') {
        // Make sure it's not := and not a URL scheme (http:// https://)
        if pos > 0
            && (pos + 1 >= s.len() || &s[pos + 1..pos + 2] != "=")
            && !s[..pos].contains("//")
            && s[..pos]
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            let key = &s[..pos];
            let val = &s[pos + 1..].trim_start();
            return Some(ItemType::Header(key.to_string(), val.to_string()));
        }
    }
    None
}

fn parse_http_args(args: &[String]) -> std::result::Result<HttpConfig, String> {
    let mut json_mode = false;
    let mut form_mode = false;
    let mut verbose = false;
    let mut headers_only = false;
    let mut output_file = None;
    let mut method: Option<String> = None;
    let mut url: Option<String> = None;
    let mut items = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--json" | "-j" => json_mode = true,
            "--form" | "-f" => form_mode = true,
            "-v" | "--verbose" => verbose = true,
            "-h" | "--headers" => headers_only = true,
            "-b" | "--body" => { /* default, no-op */ }
            "-o" | "--output" => {
                i += 1;
                if i >= args.len() {
                    return Err("http: -o requires an argument".to_string());
                }
                output_file = Some(args[i].clone());
            }
            _ if arg.starts_with('-') && url.is_none() => {
                return Err(format!("http: unknown option '{arg}'"));
            }
            _ => {
                // Positional: METHOD, URL, or item
                if url.is_none() {
                    if is_http_method(arg) && method.is_none() {
                        method = Some(arg.to_uppercase());
                    } else {
                        url = Some(arg.clone());
                    }
                } else {
                    // Try to parse as item
                    match parse_item(arg) {
                        Some(item) => items.push(item),
                        None => {
                            return Err(format!("http: invalid item '{arg}'"));
                        }
                    }
                }
            }
        }
        i += 1;
    }

    let url = url.ok_or_else(|| "http: missing URL".to_string())?;

    // Determine method: if data items present and no explicit method, use POST
    let has_data = items.iter().any(|item| {
        matches!(
            item,
            ItemType::JsonField(_, _) | ItemType::JsonRawField(_, _)
        )
    });
    let method = method.unwrap_or_else(|| {
        if has_data {
            "POST".to_string()
        } else {
            "GET".to_string()
        }
    });

    if json_mode && form_mode {
        return Err("http: --json and --form are mutually exclusive".to_string());
    }

    Ok(HttpConfig {
        method,
        url,
        items,
        json_mode,
        form_mode,
        verbose,
        headers_only,
        output_file,
    })
}

/// Build query string from query params.
fn build_url_with_query(base_url: &str, items: &[ItemType]) -> String {
    let params: Vec<String> = items
        .iter()
        .filter_map(|item| {
            if let ItemType::QueryParam(k, v) = item {
                Some(format!("{}={}", k, v))
            } else {
                None
            }
        })
        .collect();
    if params.is_empty() {
        return base_url.to_string();
    }
    let sep = if base_url.contains('?') { "&" } else { "?" };
    format!("{}{}{}", base_url, sep, params.join("&"))
}

/// Build JSON body from items.
fn build_json_body(items: &[ItemType]) -> String {
    let mut fields = Vec::new();
    for item in items {
        match item {
            ItemType::JsonField(k, v) => {
                fields.push(format!("  \"{}\": \"{}\"", k, v));
            }
            ItemType::JsonRawField(k, v) => {
                fields.push(format!("  \"{}\": {}", k, v));
            }
            _ => {}
        }
    }
    if fields.is_empty() {
        return String::new();
    }
    format!("{{\n{}\n}}", fields.join(",\n"))
}

/// Build form body from items.
fn build_form_body(items: &[ItemType]) -> String {
    let pairs: Vec<String> = items
        .iter()
        .filter_map(|item| {
            if let ItemType::JsonField(k, v) = item {
                Some(format!("{}={}", k, v))
            } else {
                None
            }
        })
        .collect();
    pairs.join("&")
}

/// Format the parsed request for display.
fn format_request(config: &HttpConfig) -> String {
    let mut output = String::new();
    let url = build_url_with_query(&config.url, &config.items);

    // Request line
    output.push_str(&format!("{} {} HTTP/1.1\n", config.method, url));

    // Headers from items
    for item in &config.items {
        if let ItemType::Header(k, v) = item {
            output.push_str(&format!("{}: {}\n", k, v));
        }
    }

    // Content-Type header
    let has_data = config.items.iter().any(|item| {
        matches!(
            item,
            ItemType::JsonField(_, _) | ItemType::JsonRawField(_, _)
        )
    });
    if has_data {
        if config.form_mode {
            output.push_str("Content-Type: application/x-www-form-urlencoded\n");
        } else {
            output.push_str("Content-Type: application/json\n");
        }
    }

    output.push('\n');

    // Body
    if has_data {
        if config.form_mode {
            output.push_str(&build_form_body(&config.items));
        } else {
            output.push_str(&build_json_body(&config.items));
        }
        output.push('\n');
    }

    output
}

#[async_trait]
impl Builtin for Http {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "http: usage: http [METHOD] URL [ITEMS...]\n".to_string(),
                1,
            ));
        }

        let config = match parse_http_args(ctx.args) {
            Ok(c) => c,
            Err(e) => return Ok(ExecResult::err(format!("{e}\n"), 1)),
        };

        // Check if http_client feature is available and configured
        #[cfg(feature = "http_client")]
        {
            if let Some(http_client) = ctx.http_client {
                return execute_http_request(http_client, &config, &ctx).await;
            }
        }

        // No network - output the parsed request
        let _ = config.output_file;
        let mut output = String::new();
        output.push_str(&format_request(&config));
        if !config.verbose && !config.headers_only {
            output.push_str("http: network access not configured\n");
        }

        Ok(ExecResult::ok(output))
    }
}

/// Execute actual HTTP request when http_client feature is enabled.
#[cfg(feature = "http_client")]
async fn execute_http_request(
    http_client: &crate::network::HttpClient,
    config: &HttpConfig,
    ctx: &Context<'_>,
) -> Result<ExecResult> {
    use crate::network::Method;

    let method = match config.method.as_str() {
        "GET" => Method::Get,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "DELETE" => Method::Delete,
        "PATCH" => Method::Patch,
        "HEAD" => Method::Head,
        _ => {
            return Ok(ExecResult::err(
                format!("http: unsupported method: {}\n", config.method),
                1,
            ));
        }
    };

    let url = build_url_with_query(&config.url, &config.items);

    // Build headers
    let mut header_pairs: Vec<(String, String)> = Vec::new();
    for item in &config.items {
        if let ItemType::Header(k, v) = item {
            header_pairs.push((k.clone(), v.clone()));
        }
    }

    // Build body
    let has_data = config.items.iter().any(|item| {
        matches!(
            item,
            ItemType::JsonField(_, _) | ItemType::JsonRawField(_, _)
        )
    });
    let body_str = if has_data {
        if config.form_mode {
            header_pairs.push((
                "Content-Type".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            ));
            build_form_body(&config.items)
        } else {
            header_pairs.push(("Content-Type".to_string(), "application/json".to_string()));
            build_json_body(&config.items)
        }
    } else {
        String::new()
    };

    let body_bytes = if body_str.is_empty() {
        None
    } else {
        Some(body_str.as_bytes())
    };

    let result = http_client
        .request_with_headers(method, &url, body_bytes, &header_pairs)
        .await;

    match result {
        Ok(response) => {
            let mut output = String::new();

            if config.verbose {
                output.push_str(&format!("{} {} HTTP/1.1\n", config.method, url));
                for (k, v) in &header_pairs {
                    output.push_str(&format!("{}: {}\n", k, v));
                }
                output.push('\n');
                output.push_str(&format!("HTTP/1.1 {}\n", response.status));
                for (k, v) in &response.headers {
                    output.push_str(&format!("{}: {}\n", k, v));
                }
                output.push('\n');
            }

            if config.headers_only {
                output.push_str(&format!("HTTP/1.1 {}\n", response.status));
                for (k, v) in &response.headers {
                    output.push_str(&format!("{}: {}\n", k, v));
                }
            } else {
                output.push_str(&response.body_string());
            }

            if let Some(ref file_path) = config.output_file {
                let path = super::resolve_path(ctx.cwd, file_path);
                if let Err(e) = ctx.fs.write_file(&path, output.as_bytes()).await {
                    return Ok(ExecResult::err(
                        format!("http: failed to write to {}: {}\n", file_path, e),
                        1,
                    ));
                }
                return Ok(ExecResult::ok(String::new()));
            }

            Ok(ExecResult::ok(output))
        }
        Err(e) => Ok(ExecResult::err(format!("http: {}\n", e), 1)),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_http(args: &[&str]) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        Http.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_no_args() {
        let result = run_http(&[]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("usage"));
    }

    #[tokio::test]
    async fn test_simple_get() {
        let result = run_http(&["https://example.com/api"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("GET https://example.com/api"));
    }

    #[tokio::test]
    async fn test_explicit_method() {
        let result = run_http(&["DELETE", "https://example.com/api/1"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("DELETE https://example.com/api/1"));
    }

    #[tokio::test]
    async fn test_post_with_json_data() {
        let result = run_http(&["https://example.com/api", "name=test", "count:=42"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("POST"));
        assert!(result.stdout.contains("Content-Type: application/json"));
        assert!(result.stdout.contains("\"name\": \"test\""));
        assert!(result.stdout.contains("\"count\": 42"));
    }

    #[tokio::test]
    async fn test_custom_header() {
        let result = run_http(&[
            "GET",
            "https://example.com/api",
            "Authorization:Bearer token123",
        ])
        .await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Authorization: Bearer token123"));
    }

    #[tokio::test]
    async fn test_query_params() {
        let result = run_http(&["https://example.com/search", "q==rust", "page==1"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("q=rust"));
        assert!(result.stdout.contains("page=1"));
    }

    #[tokio::test]
    async fn test_form_mode() {
        let result = run_http(&[
            "--form",
            "POST",
            "https://example.com/login",
            "user=admin",
            "pass=secret",
        ])
        .await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("application/x-www-form-urlencoded"));
        assert!(result.stdout.contains("user=admin&pass=secret"));
    }

    #[tokio::test]
    async fn test_json_and_form_mutually_exclusive() {
        let result = run_http(&["--json", "--form", "https://example.com/api", "key=val"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("mutually exclusive"));
    }

    #[tokio::test]
    async fn test_missing_url() {
        let result = run_http(&["GET"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing URL"));
    }

    #[tokio::test]
    async fn test_unknown_option() {
        let result = run_http(&["--unknown", "https://example.com"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("unknown option"));
    }

    #[tokio::test]
    async fn test_network_not_configured_message() {
        let result = run_http(&["https://example.com/api"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("network access not configured"));
    }

    #[tokio::test]
    async fn test_missing_o_argument() {
        let result = run_http(&["-o"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("-o requires an argument"));
    }

    #[test]
    fn test_parse_item_json_field() {
        assert_eq!(
            parse_item("name=value"),
            Some(ItemType::JsonField("name".to_string(), "value".to_string()))
        );
    }

    #[test]
    fn test_parse_item_raw_field() {
        assert_eq!(
            parse_item("count:=42"),
            Some(ItemType::JsonRawField(
                "count".to_string(),
                "42".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_item_header() {
        assert_eq!(
            parse_item("Accept:application/json"),
            Some(ItemType::Header(
                "Accept".to_string(),
                "application/json".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_item_query() {
        assert_eq!(
            parse_item("q==search term"),
            Some(ItemType::QueryParam(
                "q".to_string(),
                "search term".to_string()
            ))
        );
    }
}
