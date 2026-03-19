//! mapfile/readarray builtin — read lines from stdin into an array.
//!
//! Mutates arrays via [`BuiltinSideEffect::SetIndexedArray`](super::BuiltinSideEffect).

use async_trait::async_trait;

use super::{Builtin, BuiltinSideEffect, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// `mapfile`/`readarray` builtin — read lines from stdin into an indexed array.
///
/// Usage: mapfile [-t] [ARRAY]
///
/// - `-t` — strip trailing newlines from each line
/// - Default array name is `MAPFILE`
pub struct Mapfile;

#[async_trait]
impl Builtin for Mapfile {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut trim_trailing = false; // -t: strip trailing newlines
        let mut array_name = "MAPFILE".to_string();
        let mut positional = Vec::new();

        for arg in ctx.args {
            match arg.as_str() {
                "-t" => trim_trailing = true,
                a if a.starts_with('-') => {} // skip unknown flags
                _ => positional.push(arg.clone()),
            }
        }

        if let Some(name) = positional.first() {
            array_name = name.clone();
        }

        let input = ctx.stdin.unwrap_or("");

        let mut result = ExecResult::ok(String::new());

        // Always remove existing array first
        result
            .side_effects
            .push(BuiltinSideEffect::RemoveArray(array_name.clone()));

        // Split into lines and populate array
        if !input.is_empty() {
            let entries: Vec<(usize, String)> = input
                .lines()
                .enumerate()
                .map(|(idx, line)| {
                    let value = if trim_trailing {
                        line.to_string()
                    } else {
                        format!("{}\n", line)
                    };
                    (idx, value)
                })
                .collect();

            if !entries.is_empty() {
                result
                    .side_effects
                    .push(BuiltinSideEffect::SetIndexedArray {
                        name: array_name,
                        entries,
                    });
            }
        }

        Ok(result)
    }
}
