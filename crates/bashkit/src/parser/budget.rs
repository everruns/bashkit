//! Static budget validation on parsed AST before execution.
//!
//! Analyzes the AST for obviously expensive constructs and rejects them
//! before execution starts, providing descriptive error messages.

use super::ast::*;
use crate::limits::ExecutionLimits;

/// Errors from static budget validation.
#[derive(Debug, Clone, thiserror::Error)]
pub enum BudgetError {
    #[error("brace range too large: {{{start}..{end}}} produces {count} elements (max {max})")]
    BraceRangeTooLarge {
        start: i64,
        end: i64,
        count: u64,
        max: u64,
    },

    #[error("nested loop depth {depth} exceeds limit ({max})")]
    LoopNestingTooDeep { depth: usize, max: usize },

    #[error("estimated command count {estimated} exceeds limit ({max})")]
    TooManyCommands { estimated: usize, max: usize },
}

/// Maximum nesting depth for loops before static rejection.
const MAX_LOOP_NESTING: usize = 10;

/// Maximum brace range size for static rejection.
const MAX_STATIC_BRACE_RANGE: u64 = 100_000;

/// Maximum AST command node count before static rejection.
/// This is intentionally high — it only catches scripts with thousands of
/// top-level commands, not reasonable scripts that merely exceed runtime limits.
const MAX_AST_COMMANDS: usize = 50_000;

/// Validate an AST against execution limits before running it.
///
/// Returns `Ok(())` if the script looks safe to execute, or a descriptive
/// error explaining why it was rejected.
pub fn validate(script: &Script, _limits: &ExecutionLimits) -> Result<(), BudgetError> {
    let mut ctx = ValidationContext {
        loop_depth: 0,
        command_count: 0,
    };
    validate_commands(&script.commands, &mut ctx)
}

struct ValidationContext {
    loop_depth: usize,
    command_count: usize,
}

fn validate_commands(commands: &[Command], ctx: &mut ValidationContext) -> Result<(), BudgetError> {
    for cmd in commands {
        validate_command(cmd, ctx)?;
    }
    Ok(())
}

fn validate_command(cmd: &Command, ctx: &mut ValidationContext) -> Result<(), BudgetError> {
    ctx.command_count += 1;
    if ctx.command_count > MAX_AST_COMMANDS {
        return Err(BudgetError::TooManyCommands {
            estimated: ctx.command_count,
            max: MAX_AST_COMMANDS,
        });
    }

    match cmd {
        Command::Simple(simple) => validate_simple(simple, ctx),
        Command::Pipeline(pipeline) => validate_commands(&pipeline.commands, ctx),
        Command::List(list) => {
            validate_command(&list.first, ctx)?;
            for (_, cmd) in &list.rest {
                validate_command(cmd, ctx)?;
            }
            Ok(())
        }
        Command::Compound(compound, _redirects) => validate_compound(compound, ctx),
        Command::Function(func) => validate_command(&func.body, ctx),
    }
}

fn validate_simple(simple: &SimpleCommand, ctx: &mut ValidationContext) -> Result<(), BudgetError> {
    // Check words for expensive brace ranges
    validate_word(&simple.name, ctx)?;
    for arg in &simple.args {
        validate_word(arg, ctx)?;
    }
    Ok(())
}

fn validate_compound(
    compound: &CompoundCommand,
    ctx: &mut ValidationContext,
) -> Result<(), BudgetError> {
    match compound {
        CompoundCommand::If(if_cmd) => {
            validate_commands(&if_cmd.condition, ctx)?;
            validate_commands(&if_cmd.then_branch, ctx)?;
            for (cond, body) in &if_cmd.elif_branches {
                validate_commands(cond, ctx)?;
                validate_commands(body, ctx)?;
            }
            if let Some(else_branch) = &if_cmd.else_branch {
                validate_commands(else_branch, ctx)?;
            }
            Ok(())
        }
        CompoundCommand::For(for_cmd) => {
            ctx.loop_depth += 1;
            if ctx.loop_depth > MAX_LOOP_NESTING {
                return Err(BudgetError::LoopNestingTooDeep {
                    depth: ctx.loop_depth,
                    max: MAX_LOOP_NESTING,
                });
            }
            if let Some(words) = &for_cmd.words {
                for w in words {
                    validate_word(w, ctx)?;
                }
            }
            let result = validate_commands(&for_cmd.body, ctx);
            ctx.loop_depth -= 1;
            result
        }
        CompoundCommand::ArithmeticFor(afor) => {
            ctx.loop_depth += 1;
            if ctx.loop_depth > MAX_LOOP_NESTING {
                return Err(BudgetError::LoopNestingTooDeep {
                    depth: ctx.loop_depth,
                    max: MAX_LOOP_NESTING,
                });
            }
            let result = validate_commands(&afor.body, ctx);
            ctx.loop_depth -= 1;
            result
        }
        CompoundCommand::While(while_cmd) => {
            ctx.loop_depth += 1;
            if ctx.loop_depth > MAX_LOOP_NESTING {
                return Err(BudgetError::LoopNestingTooDeep {
                    depth: ctx.loop_depth,
                    max: MAX_LOOP_NESTING,
                });
            }
            validate_commands(&while_cmd.condition, ctx)?;
            let result = validate_commands(&while_cmd.body, ctx);
            ctx.loop_depth -= 1;
            result
        }
        CompoundCommand::Until(until_cmd) => {
            ctx.loop_depth += 1;
            if ctx.loop_depth > MAX_LOOP_NESTING {
                return Err(BudgetError::LoopNestingTooDeep {
                    depth: ctx.loop_depth,
                    max: MAX_LOOP_NESTING,
                });
            }
            validate_commands(&until_cmd.condition, ctx)?;
            let result = validate_commands(&until_cmd.body, ctx);
            ctx.loop_depth -= 1;
            result
        }
        CompoundCommand::Select(select_cmd) => {
            ctx.loop_depth += 1;
            if ctx.loop_depth > MAX_LOOP_NESTING {
                return Err(BudgetError::LoopNestingTooDeep {
                    depth: ctx.loop_depth,
                    max: MAX_LOOP_NESTING,
                });
            }
            for w in &select_cmd.words {
                validate_word(w, ctx)?;
            }
            let result = validate_commands(&select_cmd.body, ctx);
            ctx.loop_depth -= 1;
            result
        }
        CompoundCommand::Case(case_cmd) => {
            validate_word(&case_cmd.word, ctx)?;
            for item in &case_cmd.cases {
                validate_commands(&item.commands, ctx)?;
            }
            Ok(())
        }
        CompoundCommand::Subshell(commands) | CompoundCommand::BraceGroup(commands) => {
            validate_commands(commands, ctx)
        }
        CompoundCommand::Time(time_cmd) => {
            if let Some(cmd) = &time_cmd.command {
                validate_command(cmd, ctx)?;
            }
            Ok(())
        }
        CompoundCommand::Coproc(coproc) => validate_command(&coproc.body, ctx),
        CompoundCommand::Arithmetic(_) | CompoundCommand::Conditional(_) => Ok(()),
    }
}

fn validate_word(word: &Word, ctx: &mut ValidationContext) -> Result<(), BudgetError> {
    // Check for literal brace ranges that would be too large
    if !word.quoted {
        for part in &word.parts {
            validate_word_part(part, ctx)?;
        }

        // Also check if the whole word is a literal brace range like {1..999999}
        if word.parts.len() == 1
            && let WordPart::Literal(s) = &word.parts[0]
        {
            check_brace_range(s)?;
        }
    }
    Ok(())
}

fn validate_word_part(part: &WordPart, ctx: &mut ValidationContext) -> Result<(), BudgetError> {
    match part {
        WordPart::CommandSubstitution(commands) => validate_commands(commands, ctx),
        WordPart::ProcessSubstitution { commands, .. } => validate_commands(commands, ctx),
        WordPart::Literal(s) => check_brace_range(s),
        _ => Ok(()),
    }
}

/// Check if a literal string contains a brace range that would expand too large.
fn check_brace_range(s: &str) -> Result<(), BudgetError> {
    // Look for {N..M} patterns
    if !s.contains("..") || !s.contains('{') {
        return Ok(());
    }

    // Simple scan for {start..end} or {start..end..step}
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(close) = s[i..].find('}') {
                let content = &s[i + 1..i + close];
                if let Some(range_size) = estimate_brace_range_size(content)
                    && range_size > MAX_STATIC_BRACE_RANGE
                {
                    let parts: Vec<&str> = content.splitn(3, "..").collect();
                    let start = parts[0].parse::<i64>().unwrap_or(0);
                    let end = parts
                        .get(1)
                        .and_then(|s| s.parse::<i64>().ok())
                        .unwrap_or(0);
                    return Err(BudgetError::BraceRangeTooLarge {
                        start,
                        end,
                        count: range_size,
                        max: MAX_STATIC_BRACE_RANGE,
                    });
                }
                i += close + 1;
            } else {
                break;
            }
        } else {
            i += 1;
        }
    }
    Ok(())
}

/// Estimate the number of elements a brace range would produce.
/// Returns None if this doesn't look like a numeric/char range.
fn estimate_brace_range_size(content: &str) -> Option<u64> {
    let parts: Vec<&str> = content.splitn(3, "..").collect();
    if parts.len() < 2 {
        return None;
    }

    // Try numeric range
    if let (Ok(start), Ok(end)) = (parts[0].parse::<i64>(), parts[1].parse::<i64>()) {
        let step = if parts.len() == 3 {
            parts[2].parse::<i64>().ok()?.unsigned_abs().max(1)
        } else {
            1
        };
        let range = (end - start).unsigned_abs();
        return Some(range / step + 1);
    }

    // Try single-char range
    if parts[0].len() == 1 && parts[1].len() == 1 {
        let start = parts[0].as_bytes()[0];
        let end = parts[1].as_bytes()[0];
        let range = if end >= start {
            (end - start) as u64
        } else {
            (start - end) as u64
        };
        let step = if parts.len() == 3 {
            parts[2].parse::<u64>().ok()?.max(1)
        } else {
            1
        };
        return Some(range / step + 1);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    fn parse(script: &str) -> Script {
        Parser::new(script).parse().unwrap()
    }

    #[test]
    fn accepts_simple_script() {
        let ast = parse("echo hello; echo world");
        let limits = ExecutionLimits::default();
        assert!(validate(&ast, &limits).is_ok());
    }

    #[test]
    fn accepts_reasonable_for_loop() {
        let ast = parse("for i in 1 2 3; do echo $i; done");
        let limits = ExecutionLimits::default();
        assert!(validate(&ast, &limits).is_ok());
    }

    #[test]
    fn accepts_reasonable_brace_range() {
        let ast = parse("echo {1..100}");
        let limits = ExecutionLimits::default();
        assert!(validate(&ast, &limits).is_ok());
    }

    #[test]
    fn rejects_huge_brace_range() {
        let ast = parse("echo {1..999999999}");
        let limits = ExecutionLimits::default();
        let err = validate(&ast, &limits).unwrap_err();
        assert!(matches!(err, BudgetError::BraceRangeTooLarge { .. }));
    }

    #[test]
    fn rejects_deeply_nested_loops() {
        // 11 nested for loops
        let mut script = String::new();
        for i in 0..11 {
            script.push_str(&format!("for x{i} in a b; do "));
        }
        script.push_str("echo deep; ");
        for _ in 0..11 {
            script.push_str("done; ");
        }
        let ast = parse(&script);
        let limits = ExecutionLimits::default();
        let err = validate(&ast, &limits).unwrap_err();
        assert!(matches!(err, BudgetError::LoopNestingTooDeep { .. }));
    }

    #[test]
    fn accepts_10_nested_loops() {
        let mut script = String::new();
        for i in 0..10 {
            script.push_str(&format!("for x{i} in a; do "));
        }
        script.push_str("echo ok; ");
        for _ in 0..10 {
            script.push_str("done; ");
        }
        let ast = parse(&script);
        let limits = ExecutionLimits::default();
        assert!(validate(&ast, &limits).is_ok());
    }

    #[test]
    fn rejects_too_many_command_nodes() {
        // Directly test the validation with an artificially large AST
        use super::super::span::Span;
        let mut commands = Vec::new();
        for _ in 0..50_001 {
            commands.push(Command::Simple(SimpleCommand {
                name: Word::literal("true"),
                args: vec![],
                redirects: vec![],
                assignments: vec![],
                span: Span::new(),
            }));
        }
        let ast = Script {
            commands,
            span: Span::new(),
        };
        let limits = ExecutionLimits::default();
        let err = validate(&ast, &limits).unwrap_err();
        assert!(matches!(err, BudgetError::TooManyCommands { .. }));
    }

    #[test]
    fn accepts_commands_within_limit() {
        let ast = parse("echo 1; echo 2; echo 3");
        let limits = ExecutionLimits::default();
        assert!(validate(&ast, &limits).is_ok());
    }

    #[test]
    fn brace_range_with_step() {
        let ast = parse("echo {1..1000000..1000}");
        let limits = ExecutionLimits::default();
        // 1000 elements with step 1000 — should pass
        assert!(validate(&ast, &limits).is_ok());
    }

    #[test]
    fn quoted_brace_range_skipped() {
        let ast = parse(r#"echo "{1..999999999}""#);
        let limits = ExecutionLimits::default();
        // Quoted — no brace expansion
        assert!(validate(&ast, &limits).is_ok());
    }

    #[test]
    fn estimate_numeric_range() {
        assert_eq!(estimate_brace_range_size("1..10"), Some(10));
        assert_eq!(estimate_brace_range_size("1..100"), Some(100));
        assert_eq!(estimate_brace_range_size("-5..5"), Some(11));
        assert_eq!(estimate_brace_range_size("1..100..10"), Some(10));
    }

    #[test]
    fn estimate_char_range() {
        assert_eq!(estimate_brace_range_size("a..z"), Some(26));
        assert_eq!(estimate_brace_range_size("A..Z"), Some(26));
    }

    #[test]
    fn estimate_non_range() {
        assert_eq!(estimate_brace_range_size("hello"), None);
        assert_eq!(estimate_brace_range_size("a,b,c"), None);
    }

    #[test]
    fn command_substitution_in_word_checked() {
        // Command substitution with nested loops should be checked
        let ast = parse("echo $(for i in {1..999999999}; do echo $i; done)");
        let limits = ExecutionLimits::default();
        let err = validate(&ast, &limits).unwrap_err();
        assert!(matches!(err, BudgetError::BraceRangeTooLarge { .. }));
    }
}
