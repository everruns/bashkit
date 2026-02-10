//! Python Scripts Example
//!
//! Demonstrates running Python code inside BashKit's virtual environment using the
//! embedded Monty interpreter. Python runs entirely in-memory with
//! resource limits. Python pathlib.Path operations are bridged to
//! BashKit's virtual filesystem.
//!
//! Run with: cargo run --features python --example python_scripts

use bashkit::Bash;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== BashKit Python Integration ===\n");

    let mut bash = Bash::builder().python().build();

    // --- 1. Inline expressions ---
    println!("--- Inline Expressions ---");
    let result = bash.exec("python3 -c \"2 ** 10\"").await?;
    println!("python3 -c \"2 ** 10\": {}", result.stdout.trim());

    // --- 2. Print statements ---
    println!("\n--- Print Statements ---");
    let result = bash
        .exec("python3 -c \"print('Hello from Python!')\"")
        .await?;
    print!("{}", result.stdout);

    // --- 3. Multiline scripts ---
    println!("\n--- Multiline Script ---");
    let result = bash
        .exec(
            r#"python3 -c "def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)
for i in range(10):
    print(f'fib({i}) = {fib(i)}')"
"#,
        )
        .await?;
    print!("{}", result.stdout);

    // --- 4. Python in pipelines ---
    println!("--- Pipeline Integration ---");
    let result = bash
        .exec(
            r#"python3 -c "for i in range(5):
    print(f'item-{i}')" | grep "item-3""#,
        )
        .await?;
    print!("grep result: {}", result.stdout);

    // --- 5. Command substitution ---
    println!("\n--- Command Substitution ---");
    let result = bash
        .exec(
            r#"count=$(python3 -c "print(len([x for x in range(100) if x % 7 == 0]))")
echo "Numbers divisible by 7 in 0-99: $count""#,
        )
        .await?;
    print!("{}", result.stdout);

    // --- 6. Script from VFS file ---
    println!("\n--- Script File (VFS) ---");
    bash.exec(
        r#"cat > /tmp/analyze.py << 'PYEOF'
data = [23, 45, 12, 67, 34, 89, 56, 78, 90, 11]
print(f"Count: {len(data)}")
print(f"Sum:   {sum(data)}")
print(f"Min:   {min(data)}")
print(f"Max:   {max(data)}")
print(f"Avg:   {sum(data) / len(data)}")
PYEOF"#,
    )
    .await?;
    let result = bash.exec("python3 /tmp/analyze.py").await?;
    print!("{}", result.stdout);

    // --- 7. Error handling ---
    println!("\n--- Error Handling ---");
    let result = bash
        .exec(
            r#"if python3 -c "1/0" 2>/dev/null; then
    echo "succeeded (unexpected)"
else
    echo "failed with exit code $?"
fi"#,
        )
        .await?;
    print!("{}", result.stdout);

    // --- 8. Bash + Python data processing ---
    println!("\n--- Mixed Bash/Python Processing ---");
    let result = bash
        .exec(
            r#"python3 -c "
scores = [('Alice', 95), ('Bob', 87), ('Charlie', 92), ('Diana', 78), ('Eve', 96)]
total = 0
best_name = ''
best_score = 0
for name, score in scores:
    total += score
    if score > best_score:
        best_score = score
        best_name = name
print(f'Total students: {len(scores)}')
print(f'Average score:  {total / len(scores)}')
print(f'Top scorer:     {best_name} ({best_score})')
""#,
        )
        .await?;
    print!("{}", result.stdout);

    // --- 9. VFS bridging: write from Python, read from bash ---
    println!("\n--- VFS: Python writes, Bash reads ---");
    let result = bash
        .exec(
            r#"python3 -c "from pathlib import Path
_ = Path('/tmp/report.txt').write_text('Score: 95\nGrade: A\n')"
echo "Reading Python's file from bash:"
cat /tmp/report.txt"#,
        )
        .await?;
    print!("{}", result.stdout);

    // --- 10. VFS bridging: write from bash, read from Python ---
    println!("\n--- VFS: Bash writes, Python reads ---");
    let result = bash
        .exec(
            r#"echo "line1" > /tmp/data.txt
echo "line2" >> /tmp/data.txt
echo "line3" >> /tmp/data.txt
python3 -c "from pathlib import Path
content = Path('/tmp/data.txt').read_text()
print(f'Lines: {len(content.strip().splitlines())}')"
"#,
        )
        .await?;
    print!("{}", result.stdout);

    // --- 11. VFS: directory listing from Python ---
    println!("\n--- VFS: Directory listing ---");
    let result = bash
        .exec(
            r#"mkdir -p /data
echo "a" > /data/one.txt
echo "b" > /data/two.txt
python3 -c "from pathlib import Path
for p in Path('/data').iterdir():
    info = p.stat()
    print(f'{p.name}: {info.st_size} bytes')"
"#,
        )
        .await?;
    print!("{}", result.stdout);

    // --- 12. Python version ---
    println!("\n--- Version ---");
    let result = bash.exec("python3 --version").await?;
    print!("{}", result.stdout);

    println!("\n=== Demo Complete ===");
    Ok(())
}
