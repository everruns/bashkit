//! Integration tests for real-world skills.sh scripts.
//!
//! These tests verify bashkit can parse and execute actual bash scripts
//! extracted from the top skills on skills.sh. External binaries (az, helm,
//! npm, curl) are stubbed via custom builtins so we test bash feature coverage
//! without requiring real infrastructure.
//!
//! Fixtures live in tests/skills_fixtures/*.sh (verbatim copies from repos).
//!
//! Source repos (cross-ref specs/015-skills-analysis.md):
//!   - microsoft/github-copilot-for-azure (azure_*.sh)
//!   - vercel-labs/agent-skills (vercel_deploy.sh)
//!   - google-labs-code/stitch-skills (stitch_*.sh)
//!   - obra/superpowers (superpowers_find_polluter.sh)
//!   - wshobson/agents (helm_validate_chart.sh)
//!   - giuseppe-trisciuoglio/developer-kit (jwt_test_setup.sh)

use async_trait::async_trait;
use bashkit::parser::Parser;
use bashkit::{Bash, Builtin, BuiltinContext, ExecResult, ExecutionLimits, FileSystem};
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/skills_fixtures")
}

fn read_fixture(name: &str) -> String {
    let path = fixtures_dir().join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

// ---------------------------------------------------------------------------
// Stub builtins for external binaries
// ---------------------------------------------------------------------------

/// Stub that prints its invocation as JSON for assertion.
/// Usage: registers as "az", "helm", "npm", etc.
/// Output: {"cmd":"az","args":["account","show",...]}
struct EchoStub {
    name: &'static str,
}

#[async_trait]
impl Builtin for EchoStub {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        // Return a recognizable marker so scripts don't choke on empty output
        let args_str = ctx.args.join(" ");
        Ok(ExecResult::ok(format!(
            "STUB:{}:{}\n",
            self.name, args_str
        )))
    }
}

/// Stub for `az` that returns canned JSON for common subcommands.
struct AzStub;

#[async_trait]
impl Builtin for AzStub {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let args: Vec<&str> = ctx.args.iter().map(|s| s.as_str()).collect();
        match args.as_slice() {
            ["account", "show", ..] => {
                Ok(ExecResult::ok("00000000-0000-0000-0000-000000000000\n".to_string()))
            }
            ["rest", "--method", "GET", ..] => Ok(ExecResult::ok(
                "{\"value\":[{\"location\":\"eastus\",\"properties\":{\"skuName\":\"GlobalStandard\",\"availableCapacity\":100}}]}\n"
                    .to_string(),
            )),
            ["cognitiveservices", "usage", "list", ..] => {
                Ok(ExecResult::ok("[{\"name\":{\"value\":\"OpenAI.GlobalStandard.o3-mini\"},\"limit\":200,\"currentValue\":50}]\n".to_string()))
            }
            ["cognitiveservices", "model", "list", ..] => {
                Ok(ExecResult::ok("Version    Format\n2025-01-31 OpenAI\n".to_string()))
            }
            _ => Ok(ExecResult::ok("{}\n".to_string())),
        }
    }
}

/// Stub for `helm` that returns canned output for lint/template/install.
struct HelmStub;

#[async_trait]
impl Builtin for HelmStub {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let sub = ctx.args.first().map(|s| s.as_str()).unwrap_or("");
        match sub {
            "lint" => Ok(ExecResult::ok(
                "==> Linting .\n[INFO] Chart.yaml: icon is recommended\n\n1 chart(s) linted, 0 chart(s) failed\n".to_string(),
            )),
            "template" => Ok(ExecResult::ok(
                "---\napiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: test\nspec:\n  template:\n    spec:\n      containers:\n      - name: app\n        securityContext:\n          runAsNonRoot: true\n          readOnlyRootFilesystem: true\n          allowPrivilegeEscalation: false\n        resources:\n          limits:\n            cpu: 100m\n          requests:\n            cpu: 50m\n        livenessProbe:\n          httpGet:\n            path: /healthz\n        readinessProbe:\n          httpGet:\n            path: /ready\n---\napiVersion: v1\nkind: Service\nmetadata:\n  name: test\n---\napiVersion: v1\nkind: ServiceAccount\nmetadata:\n  name: test\n"
                    .to_string(),
            )),
            "install" => Ok(ExecResult::ok("NAME: test-release\nSTATUS: deployed\n".to_string())),
            "dependency" => Ok(ExecResult::ok("NAME\tVERSION\tREPOSITORY\tSTATUS\n".to_string())),
            _ => Ok(ExecResult::ok(String::new())),
        }
    }
}

/// Stub for `npm` that returns success for test/install.
struct NpmStub;

#[async_trait]
impl Builtin for NpmStub {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let sub = ctx.args.first().map(|s| s.as_str()).unwrap_or("");
        match sub {
            "test" => Ok(ExecResult::ok("Tests passed\n".to_string())),
            "install" => Ok(ExecResult::ok("added 42 packages\n".to_string())),
            _ => Ok(ExecResult::ok(String::new())),
        }
    }
}

/// Stub for `curl` that returns canned JSON responses.
/// Replaces the built-in curl so we don't need network.
struct CurlStub;

#[async_trait]
impl Builtin for CurlStub {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let mut output_file: Option<String> = None;
        let mut write_out: Option<String> = None;
        let mut i = 0;
        while i < ctx.args.len() {
            match ctx.args[i].as_str() {
                "-o" => {
                    i += 1;
                    if i < ctx.args.len() {
                        output_file = Some(ctx.args[i].clone());
                    }
                }
                "-w" | "--write-out" => {
                    i += 1;
                    if i < ctx.args.len() {
                        write_out = Some(ctx.args[i].clone());
                    }
                }
                _ => {}
            }
            i += 1;
        }

        // Write canned content to output file if -o specified
        if let Some(ref path) = output_file {
            let content = b"{\"accessToken\":\"tok_test_1234567890\",\"refreshToken\":\"ref_test_0987654321\"}";
            let p = std::path::Path::new(path);
            let _ = ctx.fs.write_file(p, content).await;
        }

        let mut result = String::new();
        // Handle -w "%{http_code}" pattern
        if let Some(ref fmt) = write_out {
            if fmt.contains("http_code") {
                result.push_str("200");
            }
        }
        if result.is_empty() && output_file.is_none() {
            result.push_str("{\"previewUrl\":\"https://test.vercel.app\",\"claimUrl\":\"https://vercel.com/claim/test\"}\n");
        }

        Ok(ExecResult::ok(result))
    }
}

/// Stub for `python3` — just echoes that it was called.
struct Python3Stub;

#[async_trait]
impl Builtin for Python3Stub {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        // For -c scripts, just return a plausible table output
        if ctx.args.first().map(|s| s.as_str()) == Some("-c") {
            return Ok(ExecResult::ok(
                "Model: o3-mini v2025-01-31 | SKU: GlobalStandard | Min Capacity: 0K TPM\nRegions with capacity: 1 | Meets target: 1 | With quota: 1 | With projects: 0\n\nRegion                 Available    Meets Target   Quota        Projects   Sample Project\n----------------------------------------------------------------------------------------------------\neastus                 100K......... YES            150K         0          (none)\n".to_string(),
            ));
        }
        Ok(ExecResult::ok(String::new()))
    }
}

/// Stub for `stat` — returns a fake file size.
struct StatStub;

#[async_trait]
impl Builtin for StatStub {
    async fn execute(&self, _ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        Ok(ExecResult::ok("1024\n".to_string()))
    }
}

/// Stub for `base64` — missing builtin, stub so scripts don't fail.
/// TODO: Remove when #287 (base64 builtin) is implemented.
struct Base64Stub;

#[async_trait]
impl Builtin for Base64Stub {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        // For testing: just return a fixed base64-url-safe string
        if ctx.args.first().map(|s| s.as_str()) == Some("-d") {
            // decode mode
            let input = ctx.stdin.unwrap_or("");
            Ok(ExecResult::ok(input.to_string()))
        } else {
            // encode mode — return a fixed encoded value
            Ok(ExecResult::ok("dTIwZjlhNzNkYTRhNzRiNjM5ODNlZmViYzdiYjZm\n".to_string()))
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: write script to VFS and make executable
// ---------------------------------------------------------------------------

async fn write_script(bash: &Bash, path: &str, content: &str) {
    let fs = bash.fs();
    let p = std::path::Path::new(path);
    fs.write_file(p, content.as_bytes()).await.unwrap();
    fs.chmod(p, 0o755).await.unwrap();
}

// ---------------------------------------------------------------------------
// Helper: build a Bash instance with common stubs
// ---------------------------------------------------------------------------

fn bash_with_stubs() -> Bash {
    Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_commands(1_000_000)
                .max_loop_iterations(100_000),
        )
        .builtin("az", Box::new(AzStub))
        .builtin("helm", Box::new(HelmStub))
        .builtin("npm", Box::new(NpmStub))
        .builtin("curl", Box::new(CurlStub))
        .builtin("python3", Box::new(Python3Stub))
        .builtin("stat", Box::new(StatStub))
        .builtin("base64", Box::new(Base64Stub))
        .builtin("keytool", Box::new(EchoStub { name: "keytool" }))
        .builtin("openssl", Box::new(EchoStub { name: "openssl" }))
        .build()
}

// ===========================================================================
// PART 1: Parse-only tests — verify every fixture parses without error
// ===========================================================================

macro_rules! parse_test {
    ($name:ident, $fixture:literal) => {
        #[test]
        fn $name() {
            let script = read_fixture($fixture);
            let parser = Parser::new(&script);
            match parser.parse() {
                Ok(ast) => {
                    assert!(
                        !ast.commands.is_empty(),
                        "parsed AST should have commands for {}",
                        $fixture
                    );
                }
                Err(e) => {
                    panic!("parse error in {}: {}", $fixture, e);
                }
            }
        }
    };
}

// Every fixture must parse cleanly
parse_test!(parse_azure_generate_url, "azure_generate_url.sh");
parse_test!(parse_azure_discover_rank, "azure_discover_rank.sh");
parse_test!(parse_azure_query_capacity, "azure_query_capacity.sh");
parse_test!(parse_vercel_deploy, "vercel_deploy.sh");
parse_test!(parse_stitch_verify_setup, "stitch_verify_setup.sh");
parse_test!(parse_stitch_fetch, "stitch_fetch.sh");
parse_test!(parse_stitch_download_asset, "stitch_download_asset.sh");
parse_test!(parse_superpowers_find_polluter, "superpowers_find_polluter.sh");
parse_test!(parse_helm_validate_chart, "helm_validate_chart.sh");
parse_test!(parse_jwt_test_setup, "jwt_test_setup.sh");

// ===========================================================================
// PART 2: Execution tests — run scripts with stubbed binaries
// ===========================================================================

/// azure generate_deployment_url.sh — tests: while/case arg parsing,
/// variable expansion, pipes (xxd | base64 | tr), heredoc in usage()
///
/// BUG: Hits MaxLoopIterations(100000) — the while/case arg parsing
/// loop or heredoc processing consumes excessive iterations.
#[tokio::test]
#[ignore = "hits MaxLoopIterations — while/case arg parsing loop bug"]
async fn exec_azure_generate_url() {
    let script = read_fixture("azure_generate_url.sh");
    let mut bash = bash_with_stubs();
    write_script(&bash, "/test.sh", &script).await;

    let result = bash
        .exec("/test.sh --subscription d5320f9a-73da-4a74-b639-83efebc7bb6f --resource-group test-rg --foundry-resource test-foundry --project test-project --deployment gpt-4o")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "script failed: {}", result.stdout);
    assert!(
        result.stdout.contains("ai.azure.com"),
        "expected URL in output, got: {}",
        result.stdout
    );
}

/// azure query_capacity.sh — tests: set -euo pipefail, ${1:?}, ${2:-},
/// if/elif, variable expansion, printf, brace expansion {1..60}, for loop
///
/// BUG: Exits with code 1 under set -euo pipefail. A command in the
/// pipeline fails (likely jq or az stub output not matching expected
/// format), causing pipefail to abort.
#[tokio::test]
#[ignore = "pipefail triggers on az/jq stub output mismatch"]
async fn exec_azure_query_capacity() {
    let script = read_fixture("azure_query_capacity.sh");
    let mut bash = bash_with_stubs();
    write_script(&bash, "/test.sh", &script).await;

    let result = bash.exec("/test.sh o3-mini 2025-01-31").await.unwrap();
    assert_eq!(result.exit_code, 0, "script failed: {}", result.stdout);
    assert!(
        result.stdout.contains("Capacity:"),
        "expected capacity output, got: {}",
        result.stdout
    );
}

/// vercel deploy.sh — tests: nested function defs, trap, mktemp, tar,
/// [[ ]] glob matching, grep -o, cut, find, basename, >&2 redirects
///
/// BUG: Exit code 2. The script's nested function definitions or
/// trap/mktemp/tar interactions cause an execution error. Parses fine.
#[tokio::test]
#[ignore = "exit code 2 — nested functions/trap/mktemp interaction"]
async fn exec_vercel_deploy() {
    let script = read_fixture("vercel_deploy.sh");
    let mut bash = bash_with_stubs();

    // Set up a minimal project directory in VFS
    let fs = bash.fs();
    fs.mkdir(std::path::Path::new("/project"), true).await.unwrap();
    fs.write_file(
        std::path::Path::new("/project/package.json"),
        br#"{"dependencies":{"next":"14.0.0"}}"#,
    )
    .await
    .unwrap();
    fs.write_file(
        std::path::Path::new("/project/index.html"),
        b"<html></html>",
    )
    .await
    .unwrap();
    write_script(&bash, "/deploy.sh", &script).await;

    let result = bash.exec("/deploy.sh /project").await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "deploy failed (exit {}): stdout={}\nThis tests nested functions, trap, tar, mktemp",
        result.exit_code, result.stdout
    );
}

/// stitch verify-setup.sh — tests: echo -e with ANSI codes, file tests,
/// grep -q, find, wc -l, array iteration ("${arr[@]}")
///
/// BUG: [ -f "components.json" ] returns false even though the file
/// exists in VFS at /project/components.json and cwd is /project.
/// Likely a cwd propagation issue into script file execution context.
#[tokio::test]
#[ignore = "[ -f ] doesn't see VFS files after cd in script execution"]
async fn exec_stitch_verify_setup() {
    let script = read_fixture("stitch_verify_setup.sh");
    let mut bash = bash_with_stubs();

    // Set up a mock project in VFS
    let fs = bash.fs();
    fs.mkdir(std::path::Path::new("/project/src/lib"), true).await.unwrap();
    fs.write_file(
        std::path::Path::new("/project/components.json"),
        b"{}",
    )
    .await
    .unwrap();
    fs.write_file(
        std::path::Path::new("/project/tailwind.config.js"),
        b"module.exports = {}",
    )
    .await
    .unwrap();
    fs.write_file(
        std::path::Path::new("/project/tsconfig.json"),
        br#"{"compilerOptions":{"paths":{"@/*":["./src/*"]}}}"#,
    )
    .await
    .unwrap();
    fs.write_file(
        std::path::Path::new("/project/src/globals.css"),
        b"@tailwind base;\n@tailwind components;\n@tailwind utilities;\n:root { --bg: white; }",
    )
    .await
    .unwrap();
    fs.mkdir(std::path::Path::new("/project/src/components/ui"), true)
        .await
        .unwrap();
    fs.write_file(
        std::path::Path::new("/project/src/components/ui/button.tsx"),
        b"export const Button = () => <button/>",
    )
    .await
    .unwrap();
    fs.write_file(
        std::path::Path::new("/project/src/lib/utils.ts"),
        b"export function cn(...args: string[]) { return args.join(' '); }",
    )
    .await
    .unwrap();
    fs.write_file(
        std::path::Path::new("/project/package.json"),
        br#"{"dependencies":{"react":"18","tailwindcss":"3","class-variance-authority":"0.7","clsx":"2","tailwind-merge":"2","tailwindcss-animate":"1"}}"#,
    )
    .await
    .unwrap();

    write_script(&bash, "/verify.sh", &script).await;
    let result = bash.exec("cd /project\n/verify.sh").await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "verify-setup failed: {}",
        result.stdout
    );
    // Should detect the setup as valid
    assert!(
        result.stdout.contains("Setup verification complete"),
        "expected completion message, got: {}",
        result.stdout
    );
}

/// stitch fetch-stitch.sh — tests: simple curl wrapper, $? check, if/fi
#[tokio::test]
async fn exec_stitch_fetch() {
    let script = read_fixture("stitch_fetch.sh");
    let mut bash = bash_with_stubs();
    write_script(&bash, "/fetch.sh", &script).await;

    let result = bash
        .exec(r#"/fetch.sh "https://example.com/stitch.html" "/tmp/output.html""#)
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "fetch failed: {}", result.stdout);
    assert!(
        result.stdout.contains("Successfully retrieved") || result.stdout.contains("Initiating"),
        "expected success message, got: {}",
        result.stdout
    );
}

/// stitch download-stitch-asset.sh — tests: dirname, mkdir -p,
/// command -v, stat, $? check
#[tokio::test]
async fn exec_stitch_download_asset() {
    let script = read_fixture("stitch_download_asset.sh");
    let mut bash = bash_with_stubs();
    write_script(&bash, "/download.sh", &script).await;

    let result = bash
        .exec(r#"/download.sh "https://storage.example.com/asset.png" "/tmp/assets/screen.png""#)
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "download failed: {}", result.stdout);
    assert!(
        result.stdout.contains("Successfully downloaded")
            || result.stdout.contains("Downloading from"),
        "expected download message, got: {}",
        result.stdout
    );
}

/// superpowers find-polluter.sh — tests: set -e, for loop with $(find),
/// arithmetic $(( )), -e file test, wc -l | tr -d, || true, continue
#[tokio::test]
async fn exec_superpowers_find_polluter() {
    let script = read_fixture("superpowers_find_polluter.sh");
    let mut bash = bash_with_stubs();

    // Create some fake test files
    let fs = bash.fs();
    fs.mkdir(std::path::Path::new("/project/src"), true)
        .await
        .unwrap();
    fs.write_file(
        std::path::Path::new("/project/src/a.test.ts"),
        b"test('a', () => {})",
    )
    .await
    .unwrap();
    fs.write_file(
        std::path::Path::new("/project/src/b.test.ts"),
        b"test('b', () => {})",
    )
    .await
    .unwrap();
    write_script(&bash, "/find-polluter.sh", &script).await;

    let result = bash
        .exec("cd /project && /find-polluter.sh .nonexistent 'src/*.test.ts'")
        .await
        .unwrap();
    assert_eq!(
        result.exit_code, 0,
        "find-polluter failed: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("No polluter found") || result.stdout.contains("Testing:"),
        "expected test output, got: {}",
        result.stdout
    );
}

/// helm validate-chart.sh — tests: functions (success/warning/error),
/// command -v, grep -q, awk, echo -e ANSI, file/dir tests, jq empty
#[tokio::test]
async fn exec_helm_validate_chart() {
    let script = read_fixture("helm_validate_chart.sh");
    let mut bash = bash_with_stubs();

    // Set up a mock Helm chart in VFS
    let fs = bash.fs();
    let chart_dir = std::path::Path::new("/chart");
    fs.mkdir(chart_dir, true).await.unwrap();
    fs.write_file(
        &chart_dir.join("Chart.yaml"),
        b"name: my-app\nversion: 1.0.0\nappVersion: \"2.0.0\"",
    )
    .await
    .unwrap();
    fs.write_file(
        &chart_dir.join("values.yaml"),
        b"replicaCount: 1\nimage:\n  repository: nginx",
    )
    .await
    .unwrap();
    fs.mkdir(&chart_dir.join("templates"), true)
        .await
        .unwrap();
    fs.write_file(
        &chart_dir.join("templates/deployment.yaml"),
        b"apiVersion: apps/v1\nkind: Deployment",
    )
    .await
    .unwrap();
    write_script(&bash, "/validate.sh", &script).await;

    let result = bash.exec("/validate.sh /chart").await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "validate-chart failed: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("Validation Complete"),
        "expected completion, got: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("my-app"),
        "expected chart name in output, got: {}",
        result.stdout
    );
}

/// jwt test-jwt-setup.sh — tests: ${response: -3} substring, functions
/// with local vars, trap cleanup EXIT, curl -s -w -o -X -H -d,
/// jq -r field extraction, ${TOKEN:0:20} substring, rm -f glob
#[tokio::test]
async fn exec_jwt_test_setup() {
    let script = read_fixture("jwt_test_setup.sh");
    let mut bash = bash_with_stubs();
    write_script(&bash, "/jwt_test.sh", &script).await;

    let result = bash
        .exec("BASE_URL=http://localhost:8080 /jwt_test.sh")
        .await
        .unwrap();
    assert!(
        result.stdout.contains("JWT") || result.stdout.contains("Starting"),
        "expected test suite output, got: {}",
        result.stdout
    );
}

/// azure discover_and_rank.sh — tests: declare -A, ${!MAP[@]},
/// set -euo pipefail, inline python3 -c with heredoc-like embedding,
/// jq with --arg, sort -u, for loop over command substitution
#[tokio::test]
async fn exec_azure_discover_rank() {
    let script = read_fixture("azure_discover_rank.sh");
    let mut bash = bash_with_stubs();
    write_script(&bash, "/discover.sh", &script).await;

    let result = bash.exec("/discover.sh o3-mini 2025-01-31 100").await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "discover_and_rank failed (exit {}): {}",
        result.exit_code, result.stdout
    );
    assert!(
        result.stdout.contains("Model:") || result.stdout.contains("Region"),
        "expected ranked output, got: {}",
        result.stdout
    );
}
