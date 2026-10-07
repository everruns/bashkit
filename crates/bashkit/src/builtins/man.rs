//! man builtin - show a command's usage as a manual page.
//!
//! Decisions:
//! - bashkit ships no man pages. `man CMD` shows the command's own `--help`
//!   text, which every bashkit program builtin provides, so the page always
//!   matches what the command accepts.
//! - `--help` is only run for registered program builtins. Shell builtins
//!   (`cd`, `exit`, `read`...) use `help CMD` instead: running `exit --help`
//!   would exit the shell, and functions or aliases never get `--help`.
//! - Inside a terminal session the page opens in `less`; everywhere else it
//!   prints, like `man CMD | cat`.

use async_trait::async_trait;

use super::{Builtin, Context, ExecutionPlan, PlanDriver, PlanStep, SubCommand};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// man builtin.
pub struct Man;

const USAGE: &str = "Usage: man [SECTION] COMMAND\nShow COMMAND's usage as a manual page.\n\nInside a bashkit::terminal::Terminal session the page opens in less.\n\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n";

/// Exit status of real man when no page exists.
const NO_ENTRY: i32 = 16;

#[async_trait]
impl Builtin for Man {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(ctx.args, USAGE, Some("man (bashkit) 0.1")) {
            return Ok(r);
        }
        // Reached only when there is no plan: nothing to look up.
        Ok(ExecResult::err(
            "What manual page do you want?\nFor example, try 'man man'.\n",
            1,
        ))
    }

    async fn execution_plan(&self, ctx: &Context<'_>) -> Result<Option<ExecutionPlan>> {
        if ctx.args.iter().any(|a| a == "--help" || a == "--version") {
            return Ok(None);
        }
        // `man 1 ls`: skip a numeric section.
        let Some(name) = ctx
            .args
            .iter()
            .find(|a| !a.starts_with('-') && !a.chars().all(|c| c.is_ascii_digit()))
        else {
            return Ok(None);
        };
        let program = !super::BASH_BUILTIN_NAMES.contains(&name.as_str())
            && ctx.shell.as_ref().is_some_and(|s| s.has_builtin(name));
        #[cfg(feature = "terminal")]
        let tty = super::pager::terminal(ctx)?;
        Ok(Some(ExecutionPlan::Driver(Box::new(ManRun {
            name: name.clone(),
            step: if program { Step::Help } else { Step::ShellHelp },
            #[cfg(feature = "terminal")]
            tty,
        }))))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    /// Next: run `NAME --help`.
    Help,
    /// Next: run `help NAME`.
    ShellHelp,
    Done,
}

struct ManRun {
    name: String,
    step: Step,
    #[cfg(feature = "terminal")]
    tty: Option<crate::terminal::Tty>,
}

impl ManRun {
    fn run(&mut self, name: &str, args: Vec<String>, next: Step) -> PlanStep {
        self.step = next;
        PlanStep::Run {
            command: SubCommand {
                name: name.to_string(),
                args,
                stdin: None,
                assignments: Vec::new(),
            },
            cwd: None,
        }
    }

    async fn show(&self, text: &str) -> Result<PlanStep> {
        let page = format!("{}(1)\n\n{}", self.name.to_uppercase(), text);
        #[cfg(feature = "terminal")]
        if let Some(tty) = &self.tty {
            let title = format!("Manual page {}(1)", self.name);
            let result =
                super::pager::page_text(tty, &page, title, super::pager::Pager::Less, false)
                    .await?;
            return Ok(PlanStep::Done(result));
        }
        Ok(PlanStep::Done(ExecResult::ok(page)))
    }
}

/// Usable help text from a sub-command result, if any.
fn help_text(result: &ExecResult) -> Option<String> {
    if result.exit_code != 0 {
        return None;
    }
    let text = result.stdout.text_lossy().into_owned();
    (!text.trim().is_empty()).then_some(text)
}

#[async_trait]
impl PlanDriver for ManRun {
    async fn next(&mut self, last: Option<ExecResult>) -> Result<PlanStep> {
        if let Some(text) = last.as_ref().and_then(help_text) {
            return self.show(&text).await;
        }
        let name = self.name.clone();
        Ok(match self.step {
            Step::Help => self.run(&name, vec!["--help".into()], Step::ShellHelp),
            Step::ShellHelp => self.run("help", vec![name], Step::Done),
            Step::Done => PlanStep::Done(ExecResult::err(
                format!("No manual entry for {}\n", self.name),
                NO_ENTRY,
            )),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::Bash;

    async fn run(script: &str) -> crate::ExecResult {
        Bash::new().exec(script).await.unwrap()
    }

    #[tokio::test]
    async fn shows_program_help_as_a_page() {
        let r = run("man grep").await;
        assert_eq!(r.exit_code, 0);
        assert!(
            r.stdout.starts_with("GREP(1)\n\nUsage: grep"),
            "{}",
            r.stdout
        );
        let r = run("man 1 ls").await;
        assert!(r.stdout.starts_with("LS(1)\n"), "{}", r.stdout);
    }

    #[tokio::test]
    async fn shell_builtins_use_help_not_dash_dash_help() {
        let r = run("man exit; echo still-here").await;
        assert!(r.stdout.contains("still-here"), "{}", r.stdout);
        assert!(r.stdout.starts_with("EXIT(1)\n"), "{}", r.stdout);
    }

    #[tokio::test]
    async fn missing_page_and_usage_errors() {
        let r = run("man no-such-cmd").await;
        assert_eq!(r.exit_code, 16);
        assert_eq!(r.stderr, "No manual entry for no-such-cmd\n");
        let r = run("f() { echo ran; }; man f").await;
        assert!(!r.stdout.contains("ran"), "{}", r.stdout);
        let r = run("man").await;
        assert_eq!(r.exit_code, 1);
        assert!(run("man --help").await.stdout.starts_with("Usage: man"));
    }
}
