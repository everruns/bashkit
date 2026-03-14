// End-to-end regression tests for nested ${...} inside array subscripts.
// Requires fixes from #599 (lexer), #600 (parser), #601 (interpreter).

use bashkit::Bash;

#[tokio::test]
async fn array_access_with_nested_array_length() {
    let mut bash = Bash::new();
    let result = bash
        .exec("names=(Ava Liam Noah)\necho ${names[$RANDOM % ${#names[@]}]}")
        .await
        .unwrap();
    let out = result.stdout.trim();
    assert!(
        out == "Ava" || out == "Liam" || out == "Noah",
        "expected one of Ava/Liam/Noah, got: {out:?}"
    );
}

#[tokio::test]
async fn assignment_with_nested_array_length_in_subscript() {
    let mut bash = Bash::new();
    let result = bash
        .exec("colors=(red blue green)\ncolor=${colors[$RANDOM % ${#colors[@]}]}\necho \"$color\"")
        .await
        .unwrap();
    let out = result.stdout.trim();
    assert!(
        out == "red" || out == "blue" || out == "green",
        "expected one of red/blue/green, got: {out:?}"
    );
}

#[tokio::test]
async fn nested_array_subscript_in_arithmetic() {
    let mut bash = Bash::new();
    let result = bash
        .exec("arr=(10 20 30 40 50)\nidx=$((${arr[$RANDOM % ${#arr[@]}]} + 1))\necho \"$idx\"")
        .await
        .unwrap();
    let val: i64 = result.stdout.trim().parse().expect("should be a number");
    assert!(
        [11, 21, 31, 41, 51].contains(&val),
        "expected 11/21/31/41/51, got: {val}"
    );
}

#[tokio::test]
async fn multiple_nested_subscripts_in_loop() {
    let mut bash = Bash::new();
    let script = "names=(Ava Liam Noah Emma)\ncolors=(red blue green)\nfor i in 1 2 3; do\n  name=${names[$RANDOM % ${#names[@]}]}\n  color=${colors[$RANDOM % ${#colors[@]}]}\n  echo \"$name:$color\"\ndone";
    let result = bash.exec(script).await.unwrap();
    let lines: Vec<&str> = result.stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3, "expected 3 lines");
    for line in &lines {
        let parts: Vec<&str> = line.split(':').collect();
        assert_eq!(parts.len(), 2);
        assert!(
            ["Ava", "Liam", "Noah", "Emma"].contains(&parts[0]),
            "unexpected name: {}",
            parts[0]
        );
        assert!(
            ["red", "blue", "green"].contains(&parts[1]),
            "unexpected color: {}",
            parts[1]
        );
    }
}

/// ${#var} in arithmetic context
#[tokio::test]
async fn string_length_in_arithmetic() {
    let mut bash = Bash::new();
    let result = bash.exec("x=hello\necho $((${#x} + 1))").await.unwrap();
    assert_eq!(result.stdout.trim(), "6");
}
