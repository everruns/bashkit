//! source/. builtin stub
//!
//! The actual source/. implementation lives in the interpreter
//! (Interpreter::execute_source) which intercepts these commands before
//! they reach the builtin dispatch. This stub exists only so the builtin
//! name is registered (e.g. for `type source` lookups). It should never
//! actually execute.

use async_trait::async_trait;
use std::sync::Arc;

use super::{Builtin, Context};
use crate::error::Result;
use crate::fs::FileSystem;
use crate::interpreter::ExecResult;

/// source/. builtin stub — real execution is in Interpreter::execute_source.
pub struct Source {
    #[allow(dead_code)]
    fs: Arc<dyn FileSystem>,
}

impl Source {
    pub fn new(fs: Arc<dyn FileSystem>) -> Self {
        Self { fs }
    }
}

#[async_trait]
impl Builtin for Source {
    async fn execute(&self, _ctx: Context<'_>) -> Result<ExecResult> {
        // Unreachable: interpreter intercepts source/. before builtin dispatch.
        // If somehow reached, return error so it's obvious.
        Ok(ExecResult::err(
            "source: internal error: should be handled by interpreter",
            1,
        ))
    }
}
