//! TypeScript Scripts Example
//!
//! Demonstrates running TypeScript code inside BashKit's virtual environment
//! using the embedded ZapCode interpreter. TypeScript runs entirely in-memory
//! with resource limits. VFS operations are bridged via external function
//! suspend/resume.
//!
//! Run with: cargo run --features typescript --example typescript_scripts

use bashkit::Bash;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== BashKit TypeScript Integration ===\n");

    let mut bash = Bash::builder().typescript().build();

    // --- 1. Inline expressions ---
    println!("--- Inline Expressions ---");
    let result = bash.exec("ts -c \"2 ** 10\"").await?;
    println!("ts -c \"2 ** 10\": {}", result.stdout.trim());

    // --- 2. Console.log ---
    println!("\n--- Console.log ---");
    let result = bash
        .exec("ts -c \"console.log('Hello from TypeScript!')\"")
        .await?;
    print!("{}", result.stdout);

    // --- 3. Multiline scripts ---
    println!("\n--- Multiline Script ---");
    let result = bash
        .exec(
            r#"ts -c "const fib = (n: number): number => n <= 1 ? n : fib(n - 1) + fib(n - 2);
for (let i = 0; i < 10; i++) {
    console.log('fib(' + i + ') = ' + fib(i));
}"
"#,
        )
        .await?;
    print!("{}", result.stdout);

    // --- 4. TypeScript in pipelines ---
    println!("--- Pipeline Integration ---");
    let result = bash
        .exec(
            r#"ts -c "for (let i = 0; i < 5; i++) { console.log('item-' + i); }" | grep "item-3""#,
        )
        .await?;
    print!("grep result: {}", result.stdout);

    // --- 5. Command substitution ---
    println!("\n--- Command Substitution ---");
    let result = bash
        .exec(
            r#"count=$(ts -c "let c = 0; for (let x = 0; x < 100; x++) { if (x % 7 === 0) c++; } console.log(c)")
echo "Numbers divisible by 7 in 0-99: $count""#,
        )
        .await?;
    print!("{}", result.stdout);

    // --- 6. Script from VFS file ---
    println!("\n--- Script File (VFS) ---");
    bash.exec(
        r#"cat > /tmp/analyze.ts << 'TSEOF'
const data = [23, 45, 12, 67, 34, 89, 56, 78, 90, 11];
const sum = data.reduce((a, b) => a + b, 0);
console.log('Count: ' + data.length);
console.log('Sum:   ' + sum);
console.log('Min:   ' + Math.min(...data));
console.log('Max:   ' + Math.max(...data));
console.log('Avg:   ' + (sum / data.length));
TSEOF"#,
    )
    .await?;
    let result = bash.exec("ts /tmp/analyze.ts").await?;
    print!("{}", result.stdout);

    // --- 7. Error handling ---
    println!("\n--- Error Handling ---");
    let result = bash
        .exec(
            r#"if ts -c "throw new Error('boom')" 2>/dev/null; then
    echo "succeeded (unexpected)"
else
    echo "failed with exit code $?"
fi"#,
        )
        .await?;
    print!("{}", result.stdout);

    // --- 8. TypeScript features ---
    println!("\n--- TypeScript Features ---");
    let result = bash
        .exec(
            r#"ts -c "
const scores: Array<[string, number]> = [
    ['Alice', 95], ['Bob', 87], ['Charlie', 92], ['Diana', 78], ['Eve', 96]
];
let total = 0;
let bestName = '';
let bestScore = 0;
for (const [name, score] of scores) {
    total += score;
    if (score > bestScore) { bestScore = score; bestName = name; }
}
console.log('Total students: ' + scores.length);
console.log('Average score:  ' + (total / scores.length));
console.log('Top scorer:     ' + bestName + ' (' + bestScore + ')');
""#,
        )
        .await?;
    print!("{}", result.stdout);

    // --- 9. Async/await and destructuring ---
    println!("\n--- Async/Await + Destructuring ---");
    let result = bash
        .exec(
            r#"ts -c "
const delay = (ms: number) => new Promise(resolve => resolve(ms));
const result = await delay(100);
const { x, y } = { x: 10, y: 20 };
const [a, ...rest] = [1, 2, 3, 4];
console.log('delay:', result);
console.log('destructured:', x, y);
console.log('rest:', rest);
""#,
        )
        .await?;
    print!("{}", result.stdout);

    // --- 10. Node.js alias ---
    println!("\n--- Node.js Alias ---");
    let result = bash
        .exec("node -e \"console.log('Hello from node -e!')\"")
        .await?;
    print!("{}", result.stdout);

    // --- 11. Deno alias ---
    println!("--- Deno Alias ---");
    let result = bash
        .exec("deno -e \"console.log('Hello from deno -e!')\"")
        .await?;
    print!("{}", result.stdout);

    // --- 12. Bun alias ---
    println!("--- Bun Alias ---");
    let result = bash
        .exec("bun -e \"console.log('Hello from bun -e!')\"")
        .await?;
    print!("{}", result.stdout);

    // --- 13. VFS: write from TypeScript, read from bash ---
    println!("\n--- VFS: TypeScript writes, Bash reads ---");
    let result = bash
        .exec(
            r#"ts -c "await writeFile('/tmp/report.txt', 'Score: 95\nGrade: A\n')"
echo "Reading TypeScript's file from bash:"
cat /tmp/report.txt"#,
        )
        .await?;
    print!("{}", result.stdout);

    // --- 14. VFS: write from bash, read from TypeScript ---
    println!("\n--- VFS: Bash writes, TypeScript reads ---");
    let result = bash
        .exec(
            r#"echo "line1" > /tmp/data.txt
echo "line2" >> /tmp/data.txt
echo "line3" >> /tmp/data.txt
ts -c "const content = await readFile('/tmp/data.txt'); console.log('Lines: ' + content.trim().split('\n').length)"
"#,
        )
        .await?;
    print!("{}", result.stdout);

    // --- 15. Version ---
    println!("\n--- Version ---");
    let result = bash.exec("ts --version").await?;
    print!("{}", result.stdout);

    println!("\n=== Demo Complete ===");
    Ok(())
}
