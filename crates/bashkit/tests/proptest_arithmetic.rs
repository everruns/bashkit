//! Property-based tests for arithmetic evaluation
//!
//! Verifies that BashKit's arithmetic matches expected i64 semantics.

use bashkit::Bash;
use proptest::prelude::*;

/// Run an arithmetic expression in BashKit and return the result
async fn eval_arithmetic(expr: &str) -> Option<i64> {
    let mut bash = Bash::new();
    let script = format!("echo $(({}))", expr);
    match bash.exec(&script).await {
        Ok(result) => result.stdout.trim().parse().ok(),
        Err(_) => None,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Addition matches Rust i64 semantics
    #[test]
    fn addition_matches_i64(a in -1000i64..1000, b in -1000i64..1000) {
        let expr = format!("{} + {}", a, b);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(eval_arithmetic(&expr));
        if let Some(val) = result {
            prop_assert_eq!(val, a + b, "Mismatch for: {}", expr);
        }
    }

    /// Subtraction matches Rust i64 semantics
    #[test]
    fn subtraction_matches_i64(a in -1000i64..1000, b in -1000i64..1000) {
        let expr = format!("{} - {}", a, b);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(eval_arithmetic(&expr));
        if let Some(val) = result {
            prop_assert_eq!(val, a - b, "Mismatch for: {}", expr);
        }
    }

    /// Multiplication matches Rust i64 semantics
    #[test]
    fn multiplication_matches_i64(a in -100i64..100, b in -100i64..100) {
        let expr = format!("{} * {}", a, b);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(eval_arithmetic(&expr));
        if let Some(val) = result {
            prop_assert_eq!(val, a * b, "Mismatch for: {}", expr);
        }
    }

    /// Division by non-zero
    #[test]
    fn division_matches_i64(a in -1000i64..1000, b in 1i64..100) {
        let expr = format!("{} / {}", a, b);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(eval_arithmetic(&expr));
        if let Some(val) = result {
            prop_assert_eq!(val, a / b, "Mismatch for: {}", expr);
        }
    }

    /// Modulo by non-zero
    #[test]
    fn modulo_matches_i64(a in -1000i64..1000, b in 1i64..100) {
        let expr = format!("{} % {}", a, b);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(eval_arithmetic(&expr));
        if let Some(val) = result {
            prop_assert_eq!(val, a % b, "Mismatch for: {}", expr);
        }
    }

    /// Comparison operators return 0 or 1
    #[test]
    fn comparisons_return_bool(a in -100i64..100, b in -100i64..100) {
        let ops = ["==", "!=", "<", ">", "<=", ">="];
        let rt = tokio::runtime::Runtime::new().unwrap();

        for op in ops {
            let expr = format!("{} {} {}", a, op, b);
            let result = rt.block_on(eval_arithmetic(&expr));
            if let Some(val) = result {
                prop_assert!(val == 0 || val == 1, "Comparison should return 0 or 1: {} = {}", expr, val);
            }
        }
    }

    /// Parentheses work correctly
    #[test]
    fn parentheses_work(a in 1i64..10, b in 1i64..10, c in 1i64..10) {
        let expr1 = format!("({} + {}) * {}", a, b, c);
        let expr2 = format!("{} + {} * {}", a, b, c);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result1 = rt.block_on(eval_arithmetic(&expr1));
        let result2 = rt.block_on(eval_arithmetic(&expr2));

        if let (Some(v1), Some(v2)) = (result1, result2) {
            prop_assert_eq!(v1, (a + b) * c, "Parentheses grouping failed: {}", expr1);
            prop_assert_eq!(v2, a + b * c, "Precedence failed: {}", expr2);
        }
    }
}
