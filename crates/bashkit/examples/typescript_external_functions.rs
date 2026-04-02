//! TypeScript External Functions Example
//!
//! Demonstrates registering a host async callback that TypeScript can call.
//!
//! Run with: cargo run --features typescript --example typescript_external_functions

use bashkit::{Bash, TypeScriptExternalFnHandler, TypeScriptLimits, ZapcodeValue};
use std::sync::Arc;

#[tokio::main]
async fn main() -> bashkit::Result<()> {
    let handler: TypeScriptExternalFnHandler = Arc::new(|name, args| {
        Box::pin(async move {
            match name.as_str() {
                "add" => {
                    let a = match args.first() {
                        Some(ZapcodeValue::Int(v)) => *v as f64,
                        Some(ZapcodeValue::Float(v)) => *v,
                        _ => 0.0,
                    };
                    let b = match args.get(1) {
                        Some(ZapcodeValue::Int(v)) => *v as f64,
                        Some(ZapcodeValue::Float(v)) => *v,
                        _ => 0.0,
                    };
                    Ok(ZapcodeValue::Float(a + b))
                }
                "greet" => {
                    let name = match args.first() {
                        Some(ZapcodeValue::String(s)) => s.to_string(),
                        _ => "world".to_string(),
                    };
                    Ok(ZapcodeValue::String(Arc::from(
                        format!("Hello, {}!", name).as_str(),
                    )))
                }
                _ => Err(format!("unknown function: {name}")),
            }
        })
    });

    let mut bash = Bash::builder()
        .typescript_with_external_handler(
            TypeScriptLimits::default(),
            vec!["add".to_string(), "greet".to_string()],
            handler,
        )
        .build();

    // Call external add function
    let result = bash.exec("ts -c \"await add(20, 22)\"").await?;
    assert_eq!(result.exit_code, 0);
    println!("add(20, 22) = {}", result.stdout.trim());

    // Call external greet function
    let result = bash.exec("ts -c \"await greet('Alice')\"").await?;
    assert_eq!(result.exit_code, 0);
    println!("greet('Alice') = {}", result.stdout.trim());

    Ok(())
}
