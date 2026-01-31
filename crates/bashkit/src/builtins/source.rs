//! source builtin - execute commands from a file

use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;

use super::{Builtin, Context};
use crate::error::Result;
use crate::fs::FileSystem;
use crate::interpreter::ExecResult;
use crate::parser::Parser;

/// source/. builtin - execute commands from a file in current shell
pub struct Source {
    fs: Arc<dyn FileSystem>,
}

impl Source {
    pub fn new(fs: Arc<dyn FileSystem>) -> Self {
        Self { fs }
    }
}

#[async_trait]
impl Builtin for Source {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let filename = match ctx.args.first() {
            Some(f) => f,
            None => {
                return Ok(ExecResult::err("source: filename argument required", 1));
            }
        };

        // Read the file
        let path = Path::new(filename);
        let content = match self.fs.read_file(path).await {
            Ok(c) => String::from_utf8_lossy(&c).to_string(),
            Err(_) => {
                return Ok(ExecResult::err(
                    format!("source: {}: No such file", filename),
                    1,
                ));
            }
        };

        // Parse and return the script for the interpreter to execute
        // We store the parsed commands in a special variable for the interpreter
        let parser = Parser::new(&content);
        match parser.parse() {
            Ok(_script) => {
                // Store the script content for interpreter to execute
                // The actual execution happens in the interpreter
                ctx.variables.insert("_SOURCE_SCRIPT".to_string(), content);
                Ok(ExecResult::ok(String::new()))
            }
            Err(e) => Ok(ExecResult::err(
                format!("source: {}: parse error: {}", filename, e),
                1,
            )),
        }
    }
}
