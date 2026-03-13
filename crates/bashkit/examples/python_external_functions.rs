//! Python External Functions Example
//!
//! Demonstrates registering a host async callback that Python can call.
//!
//! Run with: cargo run --features python --example python_external_functions

use bashkit::{
    Bash, ExtFunctionResult, MontyObject, PythonExternalFnHandler, PythonLimits, Result,
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    let handler: PythonExternalFnHandler = Arc::new(|name, args, _kwargs| {
        Box::pin(async move {
            if name != "add" {
                return ExtFunctionResult::Return(MontyObject::None);
            }

            let a = match args.first() {
                Some(MontyObject::Int(v)) => *v,
                _ => 0,
            };
            let b = match args.get(1) {
                Some(MontyObject::Int(v)) => *v,
                _ => 0,
            };

            ExtFunctionResult::Return(MontyObject::Int(a + b))
        })
    });

    let mut bash = Bash::builder()
        .python_with_external_handler(PythonLimits::default(), vec!["add".to_string()], handler)
        .build();

    let result = bash.exec("python3 -c \"print(add(20, 22))\"").await?;
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "42");

    println!("external function result: {}", result.stdout.trim());
    Ok(())
}
