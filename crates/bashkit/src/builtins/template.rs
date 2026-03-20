//! template builtin - mustache/handlebars-lite template engine
//!
//! Substitutes `{{variable}}` placeholders with values from shell variables,
//! environment, or a JSON data file. Supports `{{#if var}}...{{/if}}` and
//! `{{#each var}}...{{/each}}` block helpers.

use async_trait::async_trait;

use super::{Builtin, Context, read_text_file, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// Template builtin - mustache/handlebars-lite template engine.
///
/// Usage: template [OPTIONS] [TEMPLATE_FILE]
///
/// Options:
///   -d DATA_FILE   JSON data file for variable values
///   -e             Escape HTML entities in output
///   --strict       Error on missing variables (default: empty string)
///
/// Template syntax:
///   {{var}}              - Variable substitution
///   {{#if var}}...{{/if}} - Conditional block (renders if var is truthy)
///   {{#each var}}...{{/each}} - Iteration (renders for each array element)
///
/// Variable lookup order: JSON data > shell variables > environment.
/// Reads template from TEMPLATE_FILE or stdin if no file given.
pub struct Template;

struct TemplateConfig {
    data_file: Option<String>,
    escape_html: bool,
    strict: bool,
    template_file: Option<String>,
}

fn parse_template_args(args: &[String]) -> std::result::Result<TemplateConfig, String> {
    let mut data_file = None;
    let mut escape_html = false;
    let mut strict = false;
    let mut template_file = None;
    let mut p = super::arg_parser::ArgParser::new(args);

    while !p.is_done() {
        if let Some(val) = p.flag_value("-d", "template")? {
            data_file = Some(val.to_string());
        } else if p.flag("-e") {
            escape_html = true;
        } else if p.flag("--strict") {
            strict = true;
        } else if let Some(arg) = p.current().filter(|a| a.starts_with('-')) {
            return Err(format!("template: unknown option '{}'", arg));
        } else if let Some(arg) = p.positional() {
            template_file = Some(arg.to_string());
        }
    }

    Ok(TemplateConfig {
        data_file,
        escape_html,
        strict,
        template_file,
    })
}

/// Escape HTML entities in a string.
fn escape_html_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Look up a variable value from JSON data, shell variables, or environment.
fn lookup_var(
    name: &str,
    json_data: &serde_json::Value,
    variables: &std::collections::HashMap<String, String>,
    env: &std::collections::HashMap<String, String>,
) -> Option<String> {
    // JSON data first
    if let Some(val) = json_data.get(name) {
        return Some(json_value_to_string(val));
    }
    // Shell variables
    if let Some(val) = variables.get(name) {
        return Some(val.clone());
    }
    // Environment
    if let Some(val) = env.get(name) {
        return Some(val.clone());
    }
    None
}

/// Convert a JSON value to a display string.
fn json_value_to_string(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => val.to_string(),
    }
}

/// Check if a value is truthy (non-empty, non-null, non-false).
fn is_truthy(
    name: &str,
    json_data: &serde_json::Value,
    variables: &std::collections::HashMap<String, String>,
    env: &std::collections::HashMap<String, String>,
) -> bool {
    if let Some(val) = json_data.get(name) {
        return match val {
            serde_json::Value::Null => false,
            serde_json::Value::Bool(b) => *b,
            serde_json::Value::String(s) => !s.is_empty(),
            serde_json::Value::Array(a) => !a.is_empty(),
            serde_json::Value::Number(_) => true,
            serde_json::Value::Object(_) => true,
        };
    }
    if let Some(val) = variables.get(name) {
        return !val.is_empty();
    }
    if let Some(val) = env.get(name) {
        return !val.is_empty();
    }
    false
}

// THREAT[TM-DOS-052]: Maximum recursion depth for template rendering.
// Prevents stack overflow from deeply nested {{#if}}/{{#each}} blocks.
const MAX_TEMPLATE_DEPTH: usize = 100;

/// Render a template string with the given data sources.
fn render_template(
    template: &str,
    json_data: &serde_json::Value,
    variables: &std::collections::HashMap<String, String>,
    env: &std::collections::HashMap<String, String>,
    escape: bool,
    strict: bool,
) -> std::result::Result<String, String> {
    render_template_inner(template, json_data, variables, env, escape, strict, 0)
}

fn render_template_inner(
    template: &str,
    json_data: &serde_json::Value,
    variables: &std::collections::HashMap<String, String>,
    env: &std::collections::HashMap<String, String>,
    escape: bool,
    strict: bool,
    depth: usize,
) -> std::result::Result<String, String> {
    // THREAT[TM-DOS-052]: Prevent stack overflow from deeply nested templates
    if depth > MAX_TEMPLATE_DEPTH {
        return Err(format!(
            "template: maximum nesting depth exceeded ({})",
            MAX_TEMPLATE_DEPTH
        ));
    }
    let mut output = String::new();
    let chars: Vec<char> = template.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if i + 1 < len && chars[i] == '{' && chars[i + 1] == '{' {
            i += 2;
            // Find closing }}
            let start = i;
            while i + 1 < len && !(chars[i] == '}' && chars[i + 1] == '}') {
                i += 1;
            }
            if i + 1 >= len {
                return Err("template: unclosed {{ tag".to_string());
            }
            let tag: String = chars[start..i].iter().collect();
            let tag = tag.trim();
            i += 2; // skip }}

            if let Some(block_var) = tag.strip_prefix("#if ") {
                let block_var = block_var.trim();
                // Find {{/if}}
                let end_tag = "{{/if}}";
                let rest: String = chars[i..].iter().collect();
                let end_pos = rest
                    .find(end_tag)
                    .ok_or_else(|| format!("template: missing {{{{/if}}}} for '{block_var}'"))?;
                let block_body = &rest[..end_pos];
                i += end_pos + end_tag.len();

                if is_truthy(block_var, json_data, variables, env) {
                    let rendered = render_template_inner(
                        block_body,
                        json_data,
                        variables,
                        env,
                        escape,
                        strict,
                        depth + 1,
                    )?;
                    output.push_str(&rendered);
                }
            } else if let Some(block_var) = tag.strip_prefix("#each ") {
                let block_var = block_var.trim();
                // Find {{/each}}
                let end_tag = "{{/each}}";
                let rest: String = chars[i..].iter().collect();
                let end_pos = rest
                    .find(end_tag)
                    .ok_or_else(|| format!("template: missing {{{{/each}}}} for '{block_var}'"))?;
                let block_body = &rest[..end_pos];
                i += end_pos + end_tag.len();

                if let Some(serde_json::Value::Array(items)) = json_data.get(block_var) {
                    for item in items {
                        // Replace {{.}} with current item value
                        let item_str = json_value_to_string(item);
                        let rendered_body = block_body.replace("{{.}}", &item_str);
                        let rendered = render_template_inner(
                            &rendered_body,
                            json_data,
                            variables,
                            env,
                            escape,
                            strict,
                            depth + 1,
                        )?;
                        output.push_str(&rendered);
                    }
                } else if strict {
                    return Err(format!(
                        "template: '{block_var}' is not an array or is missing"
                    ));
                }
            } else if tag.starts_with('/') {
                // Stray closing tag - error
                return Err(format!("template: unexpected closing tag '{{{tag}}}'"));
            } else {
                // Simple variable substitution
                match lookup_var(tag, json_data, variables, env) {
                    Some(val) => {
                        if escape {
                            output.push_str(&escape_html_entities(&val));
                        } else {
                            output.push_str(&val);
                        }
                    }
                    None => {
                        if strict {
                            return Err(format!("template: undefined variable '{tag}'"));
                        }
                        // Missing var -> empty string
                    }
                }
            }
        } else {
            output.push(chars[i]);
            i += 1;
        }
    }

    Ok(output)
}

#[async_trait]
impl Builtin for Template {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let config = match parse_template_args(ctx.args) {
            Ok(c) => c,
            Err(e) => return Ok(ExecResult::err(format!("{e}\n"), 1)),
        };

        // Load JSON data if provided
        let json_data = if let Some(ref data_path) = config.data_file {
            let path = resolve_path(ctx.cwd, data_path);
            let text = match read_text_file(&*ctx.fs, &path, "template").await {
                Ok(t) => t,
                Err(e) => return Ok(e),
            };
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(ExecResult::err(
                        format!("template: invalid JSON in '{}': {}\n", data_path, e),
                        1,
                    ));
                }
            }
        } else {
            serde_json::Value::Object(serde_json::Map::new())
        };

        // Load template
        let template_text = if let Some(ref tpl_path) = config.template_file {
            let path = resolve_path(ctx.cwd, tpl_path);
            match read_text_file(&*ctx.fs, &path, "template").await {
                Ok(t) => t,
                Err(e) => return Ok(e),
            }
        } else if let Some(stdin) = ctx.stdin {
            stdin.to_string()
        } else {
            return Ok(ExecResult::err(
                "template: no template file or stdin provided\n".to_string(),
                1,
            ));
        };

        // Render
        match render_template(
            &template_text,
            &json_data,
            ctx.variables,
            ctx.env,
            config.escape_html,
            config.strict,
        ) {
            Ok(rendered) => Ok(ExecResult::ok(rendered)),
            Err(e) => Ok(ExecResult::err(format!("{e}\n"), 1)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_template(
        args: &[&str],
        stdin: Option<&str>,
        env: HashMap<String, String>,
        variables: HashMap<String, String>,
        fs: Arc<InMemoryFs>,
    ) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut variables = variables;
        let mut cwd = PathBuf::from("/");
        let fs = fs as Arc<dyn crate::fs::FileSystem>;
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            shell: None,
        };
        Template.execute(ctx).await.unwrap()
    }

    fn empty_fs() -> Arc<InMemoryFs> {
        Arc::new(InMemoryFs::new())
    }

    #[tokio::test]
    async fn test_basic_variable_substitution() {
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "world".to_string());
        let result = run_template(
            &[],
            Some("Hello {{name}}!"),
            HashMap::new(),
            vars,
            empty_fs(),
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "Hello world!");
    }

    #[tokio::test]
    async fn test_env_variable_substitution() {
        let mut env = HashMap::new();
        env.insert("HOST".to_string(), "localhost".to_string());
        let result = run_template(
            &[],
            Some("server={{HOST}}"),
            env,
            HashMap::new(),
            empty_fs(),
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "server=localhost");
    }

    #[tokio::test]
    async fn test_missing_var_empty_by_default() {
        let result = run_template(
            &[],
            Some("value={{missing}}"),
            HashMap::new(),
            HashMap::new(),
            empty_fs(),
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "value=");
    }

    #[tokio::test]
    async fn test_strict_mode_error_on_missing() {
        let result = run_template(
            &["--strict"],
            Some("value={{missing}}"),
            HashMap::new(),
            HashMap::new(),
            empty_fs(),
        )
        .await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("undefined variable"));
    }

    #[tokio::test]
    async fn test_html_escaping() {
        let mut vars = HashMap::new();
        vars.insert("content".to_string(), "<b>bold</b>".to_string());
        let result = run_template(
            &["-e"],
            Some("{{content}}"),
            HashMap::new(),
            vars,
            empty_fs(),
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "&lt;b&gt;bold&lt;/b&gt;");
    }

    #[tokio::test]
    async fn test_json_data_file() {
        let fs = Arc::new(InMemoryFs::new());
        let fs_dyn = fs.clone() as Arc<dyn crate::fs::FileSystem>;
        fs_dyn
            .write_file(
                std::path::Path::new("/data.json"),
                b"{\"greeting\": \"hi\", \"target\": \"world\"}",
            )
            .await
            .unwrap();
        let result = run_template(
            &["-d", "data.json"],
            Some("{{greeting}} {{target}}"),
            HashMap::new(),
            HashMap::new(),
            fs,
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hi world");
    }

    #[tokio::test]
    async fn test_template_from_file() {
        let fs = Arc::new(InMemoryFs::new());
        let fs_dyn = fs.clone() as Arc<dyn crate::fs::FileSystem>;
        fs_dyn
            .write_file(std::path::Path::new("/tpl.txt"), b"Hello {{name}}!")
            .await
            .unwrap();
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "test".to_string());
        let result = run_template(&["tpl.txt"], None, HashMap::new(), vars, fs).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "Hello test!");
    }

    #[tokio::test]
    async fn test_if_block_truthy() {
        let mut vars = HashMap::new();
        vars.insert("show".to_string(), "yes".to_string());
        let result = run_template(
            &[],
            Some("before{{#if show}}visible{{/if}}after"),
            HashMap::new(),
            vars,
            empty_fs(),
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "beforevisibleafter");
    }

    #[tokio::test]
    async fn test_if_block_falsy() {
        let result = run_template(
            &[],
            Some("before{{#if hidden}}invisible{{/if}}after"),
            HashMap::new(),
            HashMap::new(),
            empty_fs(),
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "beforeafter");
    }

    #[tokio::test]
    async fn test_each_block() {
        let fs = Arc::new(InMemoryFs::new());
        let fs_dyn = fs.clone() as Arc<dyn crate::fs::FileSystem>;
        fs_dyn
            .write_file(
                std::path::Path::new("/data.json"),
                b"{\"items\": [\"a\", \"b\", \"c\"]}",
            )
            .await
            .unwrap();
        let result = run_template(
            &["-d", "data.json"],
            Some("{{#each items}}[{{.}}]{{/each}}"),
            HashMap::new(),
            HashMap::new(),
            fs,
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "[a][b][c]");
    }

    #[tokio::test]
    async fn test_no_template_provided() {
        let result = run_template(&[], None, HashMap::new(), HashMap::new(), empty_fs()).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("no template"));
    }

    #[tokio::test]
    async fn test_invalid_json_data_file() {
        let fs = Arc::new(InMemoryFs::new());
        let fs_dyn = fs.clone() as Arc<dyn crate::fs::FileSystem>;
        fs_dyn
            .write_file(std::path::Path::new("/bad.json"), b"not json{")
            .await
            .unwrap();
        let result = run_template(
            &["-d", "bad.json"],
            Some("{{x}}"),
            HashMap::new(),
            HashMap::new(),
            fs,
        )
        .await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid JSON"));
    }

    #[tokio::test]
    async fn test_unknown_option() {
        let result = run_template(
            &["--foo"],
            Some("text"),
            HashMap::new(),
            HashMap::new(),
            empty_fs(),
        )
        .await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("unknown option"));
    }

    #[tokio::test]
    async fn test_unclosed_tag() {
        let result = run_template(
            &[],
            Some("Hello {{name"),
            HashMap::new(),
            HashMap::new(),
            empty_fs(),
        )
        .await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("unclosed"));
    }

    #[tokio::test]
    async fn test_missing_data_file() {
        let result = run_template(
            &["-d", "nonexistent.json"],
            Some("{{x}}"),
            HashMap::new(),
            HashMap::new(),
            empty_fs(),
        )
        .await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("template:"));
    }
}
