//! clear builtin command - clear terminal screen

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The clear builtin command.
///
/// Outputs ANSI escape sequences to clear the terminal screen.
/// In virtual/non-interactive mode, outputs the escape codes as-is.
pub struct Clear;

#[async_trait]
impl Builtin for Clear {
    async fn execute(&self, _ctx: Context<'_>) -> Result<ExecResult> {
        // ESC[2J clears the screen, ESC[H moves cursor to top-left
        Ok(ExecResult::ok("\x1b[2J\x1b[H".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFs;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_clear_outputs_ansi() {
        let args: Vec<String> = Vec::new();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            shell: None,
        };
        let result = Clear.execute(ctx).await.expect("clear failed");
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("\x1b[2J"));
        assert!(result.stdout.contains("\x1b[H"));
    }
}
