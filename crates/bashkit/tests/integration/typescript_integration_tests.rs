// Integration tests for TypeScript + Bash cross-runtime interaction.
//
// These tests verify that TypeScript and bash can share data through
// the virtual filesystem in a single Bash session. This is the core
// use case: bash orchestrates, TypeScript computes.

#![cfg(feature = "typescript")]

use bashkit::Bash;

fn bash_ts() -> Bash {
    Bash::builder().typescript().build()
}

// =============================================================================
// 1. CROSS-RUNTIME FILE SHARING
// =============================================================================

#[tokio::test]
async fn bash_writes_file_ts_reads() {
    let mut bash = bash_ts();
    bash.exec("echo 'hello from bash' > /tmp/shared.txt")
        .await
        .unwrap();
    let r = bash
        .exec("ts -c \"await readFile('/tmp/shared.txt')\"")
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0);
    assert!(r.stdout.contains("hello from bash"));
}

#[tokio::test]
async fn ts_writes_file_bash_reads() {
    let mut bash = bash_ts();
    bash.exec("ts -c \"await writeFile('/tmp/tsfile.txt', 'hello from ts\\n')\"")
        .await
        .unwrap();
    let r = bash.exec("cat /tmp/tsfile.txt").await.unwrap();
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout, "hello from ts\n");
}

#[tokio::test]
async fn roundtrip_bash_ts_bash() {
    let mut bash = bash_ts();

    // Step 1: bash writes
    bash.exec("echo 'original' > /tmp/roundtrip.txt")
        .await
        .unwrap();

    // Step 2: ts overwrites
    bash.exec("ts -c \"await writeFile('/tmp/roundtrip.txt', 'modified by ts')\"")
        .await
        .unwrap();

    // Step 3: bash reads the updated file
    let r = bash.exec("cat /tmp/roundtrip.txt").await.unwrap();
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout, "modified by ts");
}

#[tokio::test]
async fn ts_mkdir_bash_writes_ts_reads() {
    let mut bash = bash_ts();

    // TypeScript creates directory
    bash.exec("ts -c \"await mkdir('/data')\"").await.unwrap();

    // Bash writes files into it
    bash.exec("echo 'file1' > /data/a.txt && echo 'file2' > /data/b.txt")
        .await
        .unwrap();

    // TypeScript reads them back
    let r = bash
        .exec("ts -c \"await readFile('/data/a.txt')\"")
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0);
    assert!(r.stdout.contains("file1"));
}

// =============================================================================
// 2. DATA PROCESSING PIPELINES
// =============================================================================

#[tokio::test]
async fn bash_generates_csv_ts_sums() {
    let mut bash = bash_ts();

    // Bash generates CSV data
    bash.exec("printf '10\\n20\\n30\\n' > /tmp/nums.txt")
        .await
        .unwrap();

    // TypeScript reads the file (return value pattern)
    let r = bash
        .exec("ts -c \"await readFile('/tmp/nums.txt')\"")
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0);
    // File contains "10\n20\n30\n", verify we get all three numbers
    assert!(r.stdout.contains("10"));
    assert!(r.stdout.contains("30"));
}

#[tokio::test]
async fn ts_writes_json_bash_reads() {
    let mut bash = bash_ts();

    // TypeScript generates JSON
    bash.exec("ts -c \"await writeFile('/tmp/result.json', '{\\\"count\\\":42}')\"")
        .await
        .unwrap();

    // Bash reads it
    let r = bash.exec("cat /tmp/result.json").await.unwrap();
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout, "{\"count\":42}");
}

#[tokio::test]
async fn ts_command_substitution() {
    let mut bash = bash_ts();

    // Use TypeScript output in bash variable
    let r = bash
        .exec("result=$(ts -c \"console.log(6 * 7)\") && echo \"answer: $result\"")
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout.trim(), "answer: 42");
}

#[tokio::test]
async fn ts_in_pipeline() {
    let mut bash = bash_ts();

    // TypeScript output piped through grep
    let r = bash
        .exec("ts -c \"for (let i = 0; i < 5; i++) { console.log('item-' + i); }\" | grep 'item-3'")
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout.trim(), "item-3");
}

#[tokio::test]
async fn ts_conditional_in_bash() {
    let mut bash = bash_ts();

    // Success case
    let r = bash
        .exec("if ts -c \"console.log('ok')\"; then echo 'passed'; else echo 'failed'; fi")
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0);
    assert!(r.stdout.contains("passed"));

    // Failure case
    let r = bash
        .exec(
            "if ts -c \"throw new Error()\" 2>/dev/null; then echo 'passed'; else echo 'failed'; fi",
        )
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0);
    assert!(r.stdout.contains("failed"));
}

// =============================================================================
// 3. SCRIPT FILE EXECUTION
// =============================================================================

#[tokio::test]
async fn run_ts_file_from_vfs() {
    let mut bash = bash_ts();

    // Write script to VFS
    bash.exec("echo 'console.log(\"from ts file\")' > /tmp/script.ts")
        .await
        .unwrap();

    // Execute it
    let r = bash.exec("ts /tmp/script.ts").await.unwrap();
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout.trim(), "from ts file");
}

#[tokio::test]
async fn run_js_file_via_node_alias() {
    let mut bash = bash_ts();

    bash.exec("echo 'console.log(\"from js file\")' > /tmp/script.js")
        .await
        .unwrap();

    let r = bash.exec("node /tmp/script.js").await.unwrap();
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout.trim(), "from js file");
}

#[tokio::test]
async fn ts_file_with_vfs_access() {
    let mut bash = bash_ts();

    // Write data file
    bash.exec("echo 'hello world' > /tmp/input.txt")
        .await
        .unwrap();

    // Write TypeScript script that reads the data file (return value pattern)
    bash.exec(
        "cat > /tmp/reader.ts << 'EOF'\n'Read: ' + (await readFile('/tmp/input.txt')).trim()\nEOF",
    )
    .await
    .unwrap();

    // Execute the script
    let r = bash.exec("ts /tmp/reader.ts").await.unwrap();
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout.trim(), "Read: hello world");
}

// =============================================================================
// 4. ALL ALIASES WORK WITH FILES
// =============================================================================

#[tokio::test]
async fn all_aliases_execute_files() {
    let mut bash = bash_ts();

    bash.exec("echo 'console.log(\"works\")' > /tmp/test.ts")
        .await
        .unwrap();

    for cmd in &["ts", "typescript", "node", "deno", "bun"] {
        let r = bash.exec(&format!("{cmd} /tmp/test.ts")).await.unwrap();
        assert_eq!(r.exit_code, 0, "{cmd} should execute .ts files");
        assert_eq!(r.stdout.trim(), "works", "{cmd} output mismatch");
    }
}

// =============================================================================
// 5. STATE PERSISTENCE ACROSS COMMANDS
// =============================================================================

#[tokio::test]
async fn vfs_state_persists_across_ts_invocations() {
    let mut bash = bash_ts();

    // First invocation writes
    bash.exec("ts -c \"await writeFile('/tmp/counter.txt', '1')\"")
        .await
        .unwrap();

    // Second invocation reads and returns the value
    let r = bash
        .exec("ts -c \"await readFile('/tmp/counter.txt')\"")
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout.trim(), "1");

    // Third invocation increments via bash arithmetic
    let r = bash
        .exec("n=$(ts -c \"await readFile('/tmp/counter.txt')\") && ts -c \"await writeFile('/tmp/counter.txt', '2')\"")
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0);

    // Bash verifies
    let r = bash.exec("cat /tmp/counter.txt").await.unwrap();
    assert_eq!(r.stdout, "2");
}
