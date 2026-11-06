//! Python External Functions Example
//!
//! Demonstrates registering a host async callback that Python can call.
//!
//! Run with: cargo run --features python --example python_external_functions

use bashkit::{
    Bash, ExtFunctionResult, MontyObject, PythonExternalFnHandler, PythonLimits, Result,
};
use std::sync::Arc;

// Monty python3 needs bashkit's per-VM metered allocator in the final binary;
// without it python3 fails closed. It only affects this binary's allocations.
#[global_allocator]
static ALLOC: bashkit::MeteredAllocator = bashkit::MeteredAllocator;

#[tokio::main]
async fn main() -> Result<()> {
    let handler: PythonExternalFnHandler = Arc::new(|name, args, _kwargs| {
        Box::pin(async move {
            if name != "add" {
                return ExtFunctionResult::Return(MontyObject::none());
            }

            // Read values through `ObjectRef` accessors.
            let int_arg = |i: usize| args.get(i).and_then(|v| v.as_ref().as_int()).unwrap_or(0);

            ExtFunctionResult::Return(MontyObject::int(int_arg(0) + int_arg(1)))
        })
    });

    let mut bash = Bash::builder()
        .python_with_external_handler(PythonLimits::default(), vec!["add".to_string()], handler)
        .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
        .build();

    let result = bash.exec("python3 -c \"print(add(20, 22))\"").await?;
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "42");

    println!("external function result: {}", result.stdout.trim());
    Ok(())
}
