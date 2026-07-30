//! Static, pre-execution introspection of a script.
//!
//! Decision: this is a narrow projection of the AST, not the AST. The parser's
//! types are internal and change with parser work; hosts that gate permissions
//! need a stable shape (see `knowledge/integrations/script-analysis.md`).
//! Decision: literal-or-null. A word that is not fully literal reports `None`
//! rather than a partial reconstruction — a half-expanded path reads like a
//! path but is not one, which is the exact trap this API removes.
//! Decision: advisory only. THREAT[TM-ESC-032]: analysis cannot see through
//! dynamic dispatch, `eval`, functions, or aliases. It reports those as
//! unknown; enforcement stays with the builtin registry, the network
//! allowlist, the mount policy, and the `before_tool` hook.
//! Decision: substitutions and function bodies are walked (and tagged via
//! [`CommandContext`]) rather than skipped, so `echo $(rm -rf /)` never looks
//! like a bare `echo`.

use crate::error::Result;
use crate::parser::Parser;
use crate::parser::{
    AssignmentValue, Command, CommandList, CompoundCommand, Redirect, RedirectKind, Script,
    SimpleCommand, Word, WordPart,
};

/// Maximum number of recorded commands plus redirects.
///
/// THREAT[TM-DOS-022]: parser depth and fuel already bound the AST; this bounds
/// the *output* so a pathological script cannot make a host allocate without
/// limit while deciding whether to run it.
pub const MAX_ANALYSIS_NODES: usize = 4096;

/// Where a command sits in the script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandContext {
    /// Runs when the script runs (possibly conditionally, e.g. an `if` branch).
    Direct,
    /// Inside `$(…)`, backticks, or a process substitution.
    Substitution,
    /// Inside a function definition — runs only if that function is called.
    FunctionBody,
}

impl CommandContext {
    /// Stable lowercase name, used by the language bindings.
    pub fn as_str(self) -> &'static str {
        match self {
            CommandContext::Direct => "direct",
            CommandContext::Substitution => "substitution",
            CommandContext::FunctionBody => "function_body",
        }
    }
}

/// Access mode implied by a redirect operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedirectMode {
    /// `<`
    Read,
    /// `>`, `>|`, `&>`
    Write,
    /// `>>`
    Append,
}

impl RedirectMode {
    /// Stable lowercase name, used by the language bindings.
    pub fn as_str(self) -> &'static str {
        match self {
            RedirectMode::Read => "read",
            RedirectMode::Write => "write",
            RedirectMode::Append => "append",
        }
    }

    /// True for modes that can create or modify a file.
    pub fn is_write(self) -> bool {
        matches!(self, RedirectMode::Write | RedirectMode::Append)
    }
}

/// A simple command found by [`analyze`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AnalyzedCommand {
    /// Command name, or `None` when it is not statically known
    /// (`$cmd`, `${arr[0]}`, `$(echo rm)`).
    pub name: Option<String>,
    /// One entry per argument; `None` when the argument is not fully literal.
    pub args: Vec<Option<String>>,
    /// Where the command sits.
    pub context: CommandContext,
    /// Names of prefix assignments (`FOO=1 cmd` → `["FOO"]`).
    pub assignments: Vec<String>,
}

impl AnalyzedCommand {
    /// True for a bare assignment (`FOO=1`), which names no command.
    ///
    /// Such a command reports `name == None` without being dynamic — nothing is
    /// hidden from analysis.
    pub fn is_assignment_only(&self) -> bool {
        self.name.is_none() && self.args.is_empty() && !self.assignments.is_empty()
    }

    /// Arguments as literals, or `None` if any argument is not statically known.
    pub fn literal_args(&self) -> Option<Vec<&str>> {
        self.args
            .iter()
            .map(|a| a.as_deref())
            .collect::<Option<Vec<_>>>()
    }
}

/// A file redirect found by [`analyze`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AnalyzedRedirect {
    /// Target path, or `None` when it is not fully literal.
    pub path: Option<String>,
    /// Access mode implied by the operator.
    pub mode: RedirectMode,
}

/// Result of [`analyze`].
///
/// See [`script_analysis_guide`](crate::script_analysis_guide) for use cases.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct ScriptAnalysis {
    /// Every simple command, in source order.
    pub commands: Vec<AnalyzedCommand>,
    /// Every file redirect target, in source order.
    pub redirects: Vec<AnalyzedRedirect>,
    /// Function names the script defines.
    pub functions: Vec<String>,
    /// Some command name is not statically known.
    pub has_dynamic_commands: bool,
    /// Script contains `$(…)`, backticks, or process substitution.
    pub has_command_substitution: bool,
    /// Script hands a script back to the interpreter: `eval`, `source`, `.`,
    /// or a nested `bash`/`sh`. The payload's commands are not visible in
    /// `commands`.
    pub has_interpreter_reentry: bool,
    /// Node budget hit — `commands` and `redirects` are incomplete.
    pub truncated: bool,
}

impl ScriptAnalysis {
    /// Distinct, statically known command names, in first-seen order.
    pub fn command_names(&self) -> Vec<&str> {
        let mut seen = Vec::new();
        for cmd in &self.commands {
            if let Some(name) = cmd.name.as_deref()
                && !seen.contains(&name)
            {
                seen.push(name);
            }
        }
        seen
    }

    /// Commands invoking `name`, in source order.
    pub fn commands_named<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a AnalyzedCommand> {
        let name = name.to_owned();
        self.commands
            .iter()
            .filter(move |c| c.name.as_deref() == Some(name.as_str()))
    }

    /// True when the script hides work from static analysis: a dynamic command
    /// name, an interpreter re-entry (`eval`/`source`/nested `bash`), or a
    /// truncated walk.
    ///
    /// Hosts gating on an allowlist should prompt (or deny) whenever this is
    /// true, regardless of what `commands` contains.
    pub fn is_opaque(&self) -> bool {
        self.has_dynamic_commands || self.has_interpreter_reentry || self.truncated
    }
}

/// Analyze a script with default parser limits.
///
/// Returns an error if the script does not parse. A parse error must not be
/// read as "no commands" — deny or prompt instead.
pub fn analyze(script: &str) -> Result<ScriptAnalysis> {
    let ast = Parser::new(script).parse()?;
    Ok(analyze_ast(&ast))
}

/// Analyze a script with explicit parser limits.
pub fn analyze_with_limits(
    script: &str,
    max_depth: usize,
    max_fuel: usize,
) -> Result<ScriptAnalysis> {
    let ast = Parser::with_limits(script, max_depth, max_fuel).parse()?;
    Ok(analyze_ast(&ast))
}

/// Analyze an already-parsed script.
pub(crate) fn analyze_ast(script: &Script) -> ScriptAnalysis {
    let mut walker = Walker::default();
    walker.walk_commands(&script.commands, CommandContext::Direct);
    walker.out
}

/// True when a command hands a script back to the interpreter: `eval`,
/// `source`, `.`, or a nested `bash`/`sh`.
///
/// The payload is a script — inline text (`bash -c '…'`), a file, or stdin — so
/// the commands it runs are invisible here even when the payload itself is a
/// literal. Every nested-shell invocation counts, rather than sniffing for
/// `-c`: flag-sniffing misses `bash $flags` and `bash script.sh`, and erring
/// toward "opaque" only ever costs a host one extra prompt.
///
/// Wrapper commands that run *other commands* named in their arguments
/// (`xargs`, `env`, `timeout`, `find -exec`) are **not** covered; a host that
/// allowlists those must treat their arguments as commands itself.
fn is_interpreter_reentry(name: Option<&str>) -> bool {
    matches!(
        name,
        Some("eval") | Some("source") | Some(".") | Some("bash") | Some("sh")
    )
}

/// Best-effort literal text of a word: `Some` only when every part is literal.
fn literal_word(word: &Word) -> Option<String> {
    let mut out = String::new();
    for part in &word.parts {
        match part {
            WordPart::Literal(s) => out.push_str(s),
            _ => return None,
        }
    }
    Some(out)
}

#[derive(Default)]
struct Walker {
    out: ScriptAnalysis,
    nodes: usize,
}

impl Walker {
    /// Returns false once the node budget is spent.
    fn budget(&mut self) -> bool {
        if self.nodes >= MAX_ANALYSIS_NODES {
            self.out.truncated = true;
            return false;
        }
        self.nodes += 1;
        true
    }

    fn walk_commands(&mut self, commands: &[Command], ctx: CommandContext) {
        for command in commands {
            self.walk_command(command, ctx);
        }
    }

    fn walk_command(&mut self, command: &Command, ctx: CommandContext) {
        match command {
            Command::Simple(simple) => self.walk_simple(simple, ctx),
            Command::Pipeline(pipeline) => self.walk_commands(&pipeline.commands, ctx),
            Command::List(list) => self.walk_list(list, ctx),
            Command::Compound(compound, redirects) => {
                self.walk_compound(compound, ctx);
                self.walk_redirects(redirects, ctx);
            }
            Command::Function(def) => {
                if !self.out.functions.iter().any(|f| f == &def.name) {
                    self.out.functions.push(def.name.clone());
                }
                self.walk_command(&def.body, CommandContext::FunctionBody);
            }
        }
    }

    fn walk_list(&mut self, list: &CommandList, ctx: CommandContext) {
        self.walk_command(&list.first, ctx);
        for (_, command) in &list.rest {
            self.walk_command(command, ctx);
        }
    }

    fn walk_compound(&mut self, compound: &CompoundCommand, ctx: CommandContext) {
        match compound {
            CompoundCommand::If(cmd) => {
                self.walk_commands(&cmd.condition, ctx);
                self.walk_commands(&cmd.then_branch, ctx);
                for (condition, body) in &cmd.elif_branches {
                    self.walk_commands(condition, ctx);
                    self.walk_commands(body, ctx);
                }
                if let Some(else_branch) = &cmd.else_branch {
                    self.walk_commands(else_branch, ctx);
                }
            }
            CompoundCommand::For(cmd) => {
                if let Some(words) = &cmd.words {
                    self.walk_words(words, ctx);
                }
                self.walk_commands(&cmd.body, ctx);
            }
            CompoundCommand::ArithmeticFor(cmd) => self.walk_commands(&cmd.body, ctx),
            CompoundCommand::While(cmd) => {
                self.walk_commands(&cmd.condition, ctx);
                self.walk_commands(&cmd.body, ctx);
            }
            CompoundCommand::Until(cmd) => {
                self.walk_commands(&cmd.condition, ctx);
                self.walk_commands(&cmd.body, ctx);
            }
            CompoundCommand::Case(cmd) => {
                self.walk_word_parts(&cmd.word, ctx);
                for item in &cmd.cases {
                    self.walk_words(&item.patterns, ctx);
                    self.walk_commands(&item.commands, ctx);
                }
            }
            CompoundCommand::Select(cmd) => {
                self.walk_words(&cmd.words, ctx);
                self.walk_commands(&cmd.body, ctx);
            }
            CompoundCommand::Subshell(commands) | CompoundCommand::BraceGroup(commands) => {
                self.walk_commands(commands, ctx)
            }
            CompoundCommand::Arithmetic(_) => {}
            CompoundCommand::Time(cmd) => {
                if let Some(command) = &cmd.command {
                    self.walk_command(command, ctx);
                }
            }
            CompoundCommand::Conditional(words) => self.walk_words(words, ctx),
            CompoundCommand::Coproc(cmd) => self.walk_command(&cmd.body, ctx),
        }
    }

    fn walk_simple(&mut self, simple: &SimpleCommand, ctx: CommandContext) {
        // Nested commands inside the name, arguments, and assignment values
        // (`$(…)`, `<(…)`) are recorded before the command itself, so the list
        // stays in source order.
        self.walk_word_parts(&simple.name, ctx);
        for arg in &simple.args {
            self.walk_word_parts(arg, ctx);
        }
        for assignment in &simple.assignments {
            match &assignment.value {
                AssignmentValue::Scalar(word) => self.walk_word_parts(word, ctx),
                AssignmentValue::Array(words) => self.walk_words(words, ctx),
            }
        }

        if !self.budget() {
            return;
        }

        // An assignment-only command (`FOO=1`) parses as a simple command whose
        // name word is empty. That is not a dynamic command — nothing is hidden
        // — so it reports `None` without setting the dynamic flag.
        let mut name = literal_word(&simple.name);
        let empty_name = name.as_deref() == Some("");
        if empty_name {
            name = None;
        }
        if name.is_none() && !empty_name {
            self.out.has_dynamic_commands = true;
        }
        if is_interpreter_reentry(name.as_deref()) {
            self.out.has_interpreter_reentry = true;
        }

        self.out.commands.push(AnalyzedCommand {
            name,
            args: simple.args.iter().map(literal_word).collect(),
            context: ctx,
            assignments: simple.assignments.iter().map(|a| a.name.clone()).collect(),
        });

        self.walk_redirects(&simple.redirects, ctx);
    }

    fn walk_redirects(&mut self, redirects: &[Redirect], ctx: CommandContext) {
        for redirect in redirects {
            let mode = match redirect.kind {
                RedirectKind::Output | RedirectKind::Clobber | RedirectKind::OutputBoth => {
                    RedirectMode::Write
                }
                RedirectKind::Append => RedirectMode::Append,
                RedirectKind::Input => RedirectMode::Read,
                // Fd duplication, here-documents and here-strings do not name a
                // file; their target is an fd number or inline text.
                RedirectKind::DupOutput
                | RedirectKind::DupInput
                | RedirectKind::HereDoc
                | RedirectKind::HereDocStrip
                | RedirectKind::HereString => {
                    self.walk_word_parts(&redirect.target, ctx);
                    continue;
                }
            };

            self.walk_word_parts(&redirect.target, ctx);
            if !self.budget() {
                return;
            }
            self.out.redirects.push(AnalyzedRedirect {
                path: literal_word(&redirect.target),
                mode,
            });
        }
    }

    fn walk_words(&mut self, words: &[Word], ctx: CommandContext) {
        for word in words {
            self.walk_word_parts(word, ctx);
        }
    }

    /// Walk commands nested inside a word (`$(…)`, `<(…)`).
    ///
    /// A substitution inside a function body stays `FunctionBody`: it is still
    /// only reachable by calling the function.
    fn walk_word_parts(&mut self, word: &Word, ctx: CommandContext) {
        let nested = if ctx == CommandContext::FunctionBody {
            CommandContext::FunctionBody
        } else {
            CommandContext::Substitution
        };
        for part in &word.parts {
            match part {
                WordPart::CommandSubstitution(commands) => {
                    self.out.has_command_substitution = true;
                    self.walk_commands(commands, nested);
                }
                WordPart::ProcessSubstitution { commands, .. } => {
                    self.out.has_command_substitution = true;
                    self.walk_commands(commands, nested);
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(script: &str) -> ScriptAnalysis {
        analyze(script).expect("script should parse")
    }

    fn names(script: &str) -> Vec<String> {
        a(script)
            .commands
            .iter()
            .map(|c| c.name.clone().unwrap_or_else(|| "<dynamic>".into()))
            .collect()
    }

    #[test]
    fn simple_command() {
        let analysis = a("echo hello world");
        assert_eq!(analysis.commands.len(), 1);
        assert_eq!(analysis.commands[0].name.as_deref(), Some("echo"));
        assert_eq!(
            analysis.commands[0].literal_args(),
            Some(vec!["hello", "world"])
        );
        assert_eq!(analysis.commands[0].context, CommandContext::Direct);
        assert!(!analysis.is_opaque());
    }

    #[test]
    fn quoted_and_concatenated_words_are_literal() {
        let analysis = a(r#"eidos record update "rec 1" 'a'b"#);
        assert_eq!(
            analysis.commands[0].literal_args(),
            Some(vec!["record", "update", "rec 1", "ab"])
        );
    }

    #[test]
    fn pipeline_and_list_operators() {
        assert_eq!(names("a | b && c || d ; e"), ["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn negated_pipeline() {
        assert_eq!(names("! grep -q x file"), ["grep"]);
    }

    #[test]
    fn compound_commands() {
        assert_eq!(
            names("if t1; then t2; elif t3; then t4; else t5; fi"),
            ["t1", "t2", "t3", "t4", "t5"]
        );
        assert_eq!(names("while c; do b; done"), ["c", "b"]);
        assert_eq!(names("until c; do b; done"), ["c", "b"]);
        assert_eq!(names("for i in 1 2; do b; done"), ["b"]);
        assert_eq!(names("for ((i=0;i<2;i++)); do b; done"), ["b"]);
        assert_eq!(names("case x in a) b ;; *) c ;; esac"), ["b", "c"]);
        assert_eq!(names("( sub )"), ["sub"]);
        assert_eq!(names("{ grp; }"), ["grp"]);
    }

    #[test]
    fn command_substitution_is_walked_and_tagged() {
        let analysis = a("echo $(rm -rf /data)");
        assert_eq!(names("echo $(rm -rf /data)"), ["rm", "echo"]);
        assert!(analysis.has_command_substitution);
        assert_eq!(analysis.commands[0].context, CommandContext::Substitution);
        assert_eq!(
            analysis.commands[0].literal_args(),
            Some(vec!["-rf", "/data"])
        );
        assert_eq!(analysis.commands[1].context, CommandContext::Direct);
    }

    #[test]
    fn backtick_substitution_is_walked() {
        let analysis = a("echo `curl http://x`");
        assert!(analysis.has_command_substitution);
        assert!(analysis.command_names().contains(&"curl"));
    }

    #[test]
    fn process_substitution_is_walked() {
        let analysis = a("diff <(cat a) <(cat b)");
        assert!(analysis.has_command_substitution);
        assert_eq!(analysis.command_names(), ["cat", "diff"]);
    }

    #[test]
    fn substitution_in_redirect_target_is_walked() {
        let analysis = a("echo hi > $(mktemp)");
        assert!(analysis.command_names().contains(&"mktemp"));
        assert_eq!(analysis.redirects[0].path, None);
    }

    #[test]
    fn substitution_in_assignment_value_is_walked() {
        let analysis = a("out=$(rm -rf /) echo hi");
        // Source order: the substitution is recorded before its command.
        assert_eq!(analysis.command_names(), ["rm", "echo"]);
        assert_eq!(analysis.commands[1].assignments, ["out"]);
    }

    #[test]
    fn dynamic_command_name_is_null_and_flagged() {
        let analysis = a("$cmd --force");
        assert_eq!(analysis.commands[0].name, None);
        assert!(analysis.has_dynamic_commands);
        assert!(analysis.is_opaque());
    }

    #[test]
    fn substituted_command_name_is_dynamic() {
        let analysis = a("$(echo rm) -rf /");
        assert!(analysis.has_dynamic_commands);
        assert!(analysis.is_opaque());
        assert_eq!(analysis.command_names(), ["echo"]);
    }

    #[test]
    fn dynamic_arguments_are_null() {
        let analysis = a(r#"rm "$target" /tmp/fixed"#);
        assert_eq!(
            analysis.commands[0].args,
            vec![None, Some("/tmp/fixed".into())]
        );
        assert_eq!(analysis.commands[0].literal_args(), None);
        // A dynamic *argument* is not a dynamic command.
        assert!(!analysis.has_dynamic_commands);
    }

    #[test]
    fn partial_literals_are_not_reconstructed() {
        let analysis = a("cat /tmp/$name.txt");
        assert_eq!(analysis.commands[0].args, vec![None]);
    }

    #[test]
    fn eval_and_source_are_flagged() {
        for script in ["eval \"$x\"", "source /tmp/x.sh", ". /tmp/x.sh"] {
            let analysis = a(script);
            assert!(
                analysis.has_interpreter_reentry,
                "{script} should set has_interpreter_reentry"
            );
            assert!(analysis.is_opaque());
        }
        assert!(!a("echo eval").has_interpreter_reentry);
    }

    #[test]
    fn nested_shell_is_flagged() {
        for script in [
            "bash -c 'rm -rf /'",
            "sh -c \"$payload\"",
            "bash --noprofile -c 'x'",
            // A script file or a computed flag hides commands just as well as
            // inline `-c` text, so every nested shell counts.
            "bash script.sh",
            "sh /tmp/setup.sh",
            "bash $flags",
        ] {
            assert!(
                a(script).has_interpreter_reentry,
                "{script} should set has_interpreter_reentry"
            );
            assert!(a(script).is_opaque(), "{script} should be opaque");
        }
        // Only the shell itself — a command whose *argument* mentions one is
        // an ordinary command.
        assert!(!a("echo bash -c hi").has_interpreter_reentry);
        assert!(!a("cat /usr/bin/bash").has_interpreter_reentry);
    }

    #[test]
    fn function_definitions_are_recorded_and_tagged() {
        let analysis = a("cleanup() { rm -rf /data; }\necho done");
        assert_eq!(analysis.functions, ["cleanup"]);
        assert_eq!(analysis.commands[0].name.as_deref(), Some("rm"));
        assert_eq!(analysis.commands[0].context, CommandContext::FunctionBody);
        assert_eq!(analysis.commands[1].context, CommandContext::Direct);
    }

    #[test]
    fn substitution_inside_function_body_stays_function_body() {
        let analysis = a("f() { echo $(rm -rf /); }");
        assert!(
            analysis
                .commands
                .iter()
                .all(|c| c.context == CommandContext::FunctionBody),
            "{:?}",
            analysis.commands
        );
    }

    #[test]
    fn redirect_modes() {
        let analysis = a("cat < in > out 2>> err");
        let modes: Vec<_> = analysis.redirects.iter().map(|r| r.mode).collect();
        assert_eq!(
            modes,
            [
                RedirectMode::Read,
                RedirectMode::Write,
                RedirectMode::Append
            ]
        );
        assert_eq!(
            analysis
                .redirects
                .iter()
                .map(|r| r.path.clone().unwrap())
                .collect::<Vec<_>>(),
            ["in", "out", "err"]
        );
        assert!(RedirectMode::Write.is_write());
        assert!(!RedirectMode::Read.is_write());
    }

    #[test]
    fn clobber_and_both_are_writes() {
        let analysis = a("echo x >| a; echo y &> b");
        assert!(analysis.redirects.iter().all(|r| r.mode.is_write()));
    }

    #[test]
    fn fd_dup_and_heredoc_are_not_file_targets() {
        let analysis = a("cmd 2>&1");
        assert!(analysis.redirects.is_empty());
        let analysis = a("cat <<EOF\nbody\nEOF\n");
        assert!(analysis.redirects.is_empty());
        let analysis = a("cat <<< hello");
        assert!(analysis.redirects.is_empty());
    }

    #[test]
    fn compound_redirects_are_recorded() {
        let analysis = a("{ echo a; } > out");
        assert_eq!(analysis.redirects[0].path.as_deref(), Some("out"));
    }

    #[test]
    fn assignments_are_recorded_without_values() {
        let analysis = a("FOO=1 BAR=2 env");
        assert_eq!(analysis.commands[0].assignments, ["FOO", "BAR"]);
    }

    #[test]
    fn bare_assignment_names_no_command_and_is_not_dynamic() {
        let analysis = a("FOO=1");
        assert_eq!(analysis.commands[0].name, None);
        assert_eq!(analysis.commands[0].assignments, ["FOO"]);
        assert!(analysis.commands[0].is_assignment_only());
        assert!(!analysis.has_dynamic_commands);
        assert!(!analysis.is_opaque());
    }

    #[test]
    fn prefix_assignment_command_is_not_assignment_only() {
        let analysis = a("FOO=1 env");
        assert!(!analysis.commands[0].is_assignment_only());
        assert_eq!(analysis.commands[0].name.as_deref(), Some("env"));
    }

    #[test]
    fn array_assignment_values_are_walked() {
        let analysis = a("arr=(a $(id) c)");
        assert!(analysis.command_names().contains(&"id"));
    }

    #[test]
    fn command_names_are_deduplicated_in_order() {
        let analysis = a("echo a; cat b; echo c");
        assert_eq!(analysis.command_names(), ["echo", "cat"]);
    }

    #[test]
    fn commands_named_filters() {
        let analysis = a("eidos record update 1; echo x; eidos doc get 2");
        let found: Vec<_> = analysis
            .commands_named("eidos")
            .map(|c| c.args[0].clone().unwrap())
            .collect();
        assert_eq!(found, ["record", "doc"]);
    }

    #[test]
    fn parse_error_is_an_error_not_empty_output() {
        assert!(analyze("if true; then").is_err());
        assert!(analyze("echo 'unterminated").is_err());
    }

    #[test]
    fn empty_script_is_empty_analysis() {
        let analysis = a("");
        assert!(analysis.commands.is_empty());
        assert!(!analysis.is_opaque());
    }

    #[test]
    fn comments_only_is_empty_analysis() {
        assert!(a("# nothing here\n").commands.is_empty());
    }

    #[test]
    fn budget_truncates_and_flags() {
        let script = "echo x;".repeat(MAX_ANALYSIS_NODES + 50);
        let analysis = a(&script);
        assert!(analysis.truncated);
        assert!(analysis.is_opaque());
        assert_eq!(analysis.commands.len(), MAX_ANALYSIS_NODES);
    }

    #[test]
    fn limits_are_honored() {
        // Fuel starvation surfaces as a parse error, not a partial analysis.
        assert!(analyze_with_limits("echo hello", 100, 2).is_err());
        assert!(analyze_with_limits("echo hello", 100, 100_000).is_ok());
    }

    #[test]
    fn analysis_is_serializable() {
        let analysis = a("echo hi > out");
        let json = serde_json::to_string(&analysis).expect("serializable");
        assert!(json.contains("\"direct\""));
        assert!(json.contains("\"write\""));
        let round: ScriptAnalysis = serde_json::from_str(&json).expect("deserializable");
        assert_eq!(round, analysis);
    }

    #[test]
    fn context_and_mode_names_are_stable() {
        assert_eq!(CommandContext::Direct.as_str(), "direct");
        assert_eq!(CommandContext::Substitution.as_str(), "substitution");
        assert_eq!(CommandContext::FunctionBody.as_str(), "function_body");
        assert_eq!(RedirectMode::Read.as_str(), "read");
        assert_eq!(RedirectMode::Write.as_str(), "write");
        assert_eq!(RedirectMode::Append.as_str(), "append");
    }
}
