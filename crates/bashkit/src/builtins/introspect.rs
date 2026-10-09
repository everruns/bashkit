//! Shell introspection builtins: type, which, hash
//!
//! These builtins query shell metadata (registered builtins, functions, keywords)
//! via [`ShellRef`](super::ShellRef) rather than direct interpreter access.

// Retained hash payloads own request-budget leases. Copy-on-write shell forks
// share strings and leases; each new host request re-admits persistent entries.
use std::sync::Arc;

use async_trait::async_trait;

use super::{
    BASH_BUILTIN_NAMES, Builtin, Context, search_path, search_path_all, search_path_or_file,
};
use crate::error::Result;
use crate::interpreter::ExecResult;
use crate::limits::{ExecutionBudget, ExecutionBudgetLease};

/// One remembered `$PATH` lookup (`hash`).
#[derive(Clone, Debug)]
pub(crate) struct HashEntry {
    pub(crate) name: Arc<str>,
    /// The file found. `None` for a registered builtin that real bash runs
    /// from `PATH` (`whoami`): its path is the root filesystem stub, looked
    /// up when shown.
    pub(crate) path: Option<Arc<str>>,
    pub(crate) hits: u32,
    _lease: Arc<ExecutionBudgetLease>,
}

/// bash's command hash table: names found along `$PATH`, the file each one
/// resolved to, and how often it ran. A hashed name runs that file without
/// searching again until `hash -r` or a `PATH` assignment clears the table.
#[derive(Clone, Debug, Default)]
pub(crate) struct CommandHash {
    entries: Vec<HashEntry>,
}

impl CommandHash {
    /// Bound on remembered names; past it the oldest entry is dropped
    /// (THREAT[TM-DOS-060]: a loop over generated names cannot grow it).
    const MAX_ENTRIES: usize = 512;

    /// THREAT[TM-DOS-129]: persistent entries cannot escape the next request's
    /// live-byte ceiling. Rebinding shares payloads and is atomic on failure.
    pub(crate) fn bind_budget(&mut self, budget: &ExecutionBudget) -> Result<()> {
        let mut entries = self.entries.clone();
        for entry in &mut entries {
            entry._lease = Arc::new(
                budget.lease_bytes(Self::entry_bytes(&entry.name, entry.path.as_deref()))?,
            );
        }
        self.entries = entries;
        Ok(())
    }

    fn entry_bytes(name: &str, path: Option<&str>) -> usize {
        name.len()
            .saturating_add(path.map_or(0, str::len))
            .saturating_add(std::mem::size_of::<HashEntry>())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    pub(crate) fn get(&self, name: &str) -> Option<&HashEntry> {
        self.entries.iter().find(|e| e.name.as_ref() == name)
    }

    pub(crate) fn remove(&mut self, name: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.name.as_ref() != name);
        before != self.entries.len()
    }

    /// Remember `name` (replacing any entry) with `hits` uses.
    pub(crate) fn insert(
        &mut self,
        name: &str,
        path: Option<&str>,
        hits: u32,
        budget: &ExecutionBudget,
    ) -> Result<()> {
        // THREAT[TM-DOS-129]: admit every retained copy before allocating it,
        // including names, arbitrary `hash -p` paths, and entry metadata.
        let lease = budget.lease_bytes(Self::entry_bytes(name, path))?;
        let entry = HashEntry {
            name: Arc::from(name),
            path: path.map(Arc::from),
            hits,
            _lease: Arc::new(lease),
        };
        self.remove(name);
        if self.entries.len() >= Self::MAX_ENTRIES {
            self.entries.remove(0);
        }
        self.entries.push(entry);
        Ok(())
    }

    /// Count one use of `name`, adding it with `path` when new.
    pub(crate) fn hit(
        &mut self,
        name: &str,
        path: Option<&str>,
        budget: &ExecutionBudget,
    ) -> Result<()> {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.name.as_ref() == name) {
            entry.hits = entry.hits.saturating_add(1);
            Ok(())
        } else {
            self.insert(name, path, 1, budget)
        }
    }

    pub(crate) fn entries(&self) -> &[HashEntry] {
        &self.entries
    }
}

/// The file a hash entry stands for, resolving a builtin's root-filesystem
/// stub on demand.
async fn hashed_path(ctx: &Context<'_>, entry: &HashEntry) -> Option<String> {
    match &entry.path {
        Some(p) => Some(p.to_string()),
        None => search_path(ctx, &entry.name).await,
    }
}

/// `type` builtin — display information about command type.
///
/// Usage: type [-afptP] name [name ...]
///
/// Follows bash's `describe_command`: alias (with `expand_aliases`), keyword,
/// function (unless `-f`), builtin, hashed file (unless `-a`), then the files
/// along `PATH` (all of them with `-a`). `-P` skips straight to the file
/// search; `-p` prints only file paths but a name found earlier still counts
/// as found.
pub struct Type;

#[derive(Clone, Copy, PartialEq)]
enum TypeMode {
    Describe,
    TypeOnly,
    PathOnly,
}

#[async_trait]
impl Builtin for Type {
    async fn execute(&self, mut ctx: Context<'_>) -> Result<ExecResult> {
        let Some(shell) = ctx.shell.as_ref() else {
            return Ok(ExecResult::err(
                "bash: type: not available in this context\n".to_string(),
                1,
            ));
        };

        let mut mode = TypeMode::Describe;
        let mut force_path = false; // -P
        let mut show_all = false; // -a
        let mut no_funcs = false; // -f
        let mut names: Vec<&str> = Vec::new();

        let mut options_done = false;
        for arg in ctx.args {
            if !options_done && arg == "--" {
                options_done = true;
            } else if !options_done && names.is_empty() && arg.starts_with('-') && arg.len() > 1 {
                for c in arg[1..].chars() {
                    match c {
                        't' => mode = TypeMode::TypeOnly,
                        'p' => mode = TypeMode::PathOnly,
                        'a' => show_all = true,
                        'f' => no_funcs = true,
                        'P' => {
                            mode = TypeMode::PathOnly;
                            force_path = true;
                        }
                        _ => {
                            return Ok(ExecResult::err(
                                format!(
                                    "bash: type: -{}: invalid option\ntype: usage: type [-afptP] name [name ...]\n",
                                    c
                                ),
                                2,
                            ));
                        }
                    }
                }
            } else {
                names.push(arg);
            }
        }

        if names.is_empty() {
            return Ok(ExecResult::ok(String::new()));
        }

        let expand_aliases = ctx
            .variables
            .get("SHOPT_expand_aliases")
            .is_some_and(|v| v == "1");

        let mut output = String::new();
        let mut errors = String::new();
        let mut all_found = true;
        let mut hashed_hits: Vec<String> = Vec::new();

        for name in &names {
            let mut found = false;
            // Each step returns early (`continue 'name`) unless `-a`.
            'name: {
                if !force_path {
                    if expand_aliases && let Some(value) = shell.aliases.get(*name) {
                        match mode {
                            TypeMode::TypeOnly => output.push_str("alias\n"),
                            TypeMode::Describe => {
                                output.push_str(&format!("{name} is aliased to `{value}'\n"))
                            }
                            TypeMode::PathOnly => {}
                        }
                        found = true;
                        if !show_all {
                            break 'name;
                        }
                    }
                    if shell.is_keyword(name) {
                        match mode {
                            TypeMode::TypeOnly => output.push_str("keyword\n"),
                            TypeMode::Describe => {
                                output.push_str(&format!("{name} is a shell keyword\n"))
                            }
                            TypeMode::PathOnly => {}
                        }
                        found = true;
                        if !show_all {
                            break 'name;
                        }
                    }
                    if !no_funcs && shell.has_function(name) {
                        match mode {
                            TypeMode::TypeOnly => output.push_str("function\n"),
                            TypeMode::Describe => {
                                output.push_str(&format!("{name} is a function\n"));
                                if let Some(text) = shell.function_text(name) {
                                    output.push_str(&text);
                                    output.push('\n');
                                }
                            }
                            TypeMode::PathOnly => {}
                        }
                        found = true;
                        if !show_all {
                            break 'name;
                        }
                    }
                    // A registered command that real bash runs from PATH is a
                    // file when the root filesystem provides it.
                    let is_builtin = shell.has_builtin(name)
                        && (BASH_BUILTIN_NAMES.contains(name)
                            || search_path(&ctx, name).await.is_none());
                    if is_builtin {
                        match mode {
                            TypeMode::TypeOnly => output.push_str("builtin\n"),
                            TypeMode::Describe => {
                                output.push_str(&format!("{name} is a shell builtin\n"))
                            }
                            TypeMode::PathOnly => {}
                        }
                        found = true;
                        if !show_all {
                            break 'name;
                        }
                    }
                }
                if (!show_all || force_path)
                    && let Some(entry) = shell.command_hash.get(name)
                    && let Some(path) = hashed_path(&ctx, entry).await
                {
                    // bash counts a lookup through `type` as a hit.
                    hashed_hits.push(name.to_string());
                    match mode {
                        TypeMode::TypeOnly => output.push_str("file\n"),
                        TypeMode::Describe => {
                            output.push_str(&format!("{name} is hashed ({path})\n"))
                        }
                        TypeMode::PathOnly => output.push_str(&format!("{path}\n")),
                    }
                    found = true;
                    break 'name;
                }
                let files = if show_all {
                    search_path_all(&ctx, name, false).await
                } else {
                    search_path_or_file(&ctx, name).await.into_iter().collect()
                };
                for path in files {
                    match mode {
                        TypeMode::TypeOnly => output.push_str("file\n"),
                        TypeMode::Describe => output.push_str(&format!("{name} is {path}\n")),
                        TypeMode::PathOnly => output.push_str(&format!("{path}\n")),
                    }
                    found = true;
                }
            }
            if !found {
                if mode == TypeMode::Describe {
                    errors.push_str(&format!("bash: type: {name}: not found\n"));
                }
                all_found = false;
            }
        }

        if let Some(shell) = ctx.shell.as_mut() {
            for name in &hashed_hits {
                shell.command_hash.hit(name, None, shell.execution_budget)?;
            }
        }
        let exit_code = if all_found { 0 } else { 1 };
        Ok(ExecResult {
            stdout: output.into(),
            stderr: errors.into(),
            exit_code,
            ..Default::default()
        })
    }
}

/// `which` builtin — locate a command.
///
/// Searches `PATH` on the VFS (the root filesystem provides `/usr/bin/NAME`
/// for registered commands). Without a root filesystem, a registered builtin
/// reports its bare name.
pub struct Which;

#[async_trait]
impl Builtin for Which {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let Some(shell) = ctx.shell.as_ref() else {
            return Ok(ExecResult::err(String::new(), 1));
        };

        if ctx.args.is_empty() {
            return Ok(ExecResult::ok(String::new()));
        }

        let mut output = String::new();
        let mut all_found = true;

        for name in ctx.args {
            if let Some(path) = search_path(&ctx, name).await {
                output.push_str(&format!("{path}\n"));
            } else if shell.has_builtin(name) && !BASH_BUILTIN_NAMES.contains(&name.as_str()) {
                // No root filesystem: the builtin is the only "executable".
                output.push_str(&format!("{}\n", name));
            } else {
                all_found = false;
            }
        }

        let exit_code = if all_found { 0 } else { 1 };
        Ok(ExecResult {
            stdout: output.into(),
            exit_code,
            ..Default::default()
        })
    }
}

/// `hash` builtin: show and edit the command hash table ([`CommandHash`]).
///
/// `hash` lists it, `hash NAME` adds NAME's `PATH` file, `-r` clears, `-d`
/// forgets, `-t` prints the file, `-l` prints reusable `hash -p` lines and
/// `-p FILE NAME` sets an entry. Shell builtins (`cd`) are never hashed.
pub struct Hash;

const HASH_USAGE: &str = "hash: usage: hash [-lr] [-p pathname] [-dt] [name ...]\n";

#[async_trait]
impl Builtin for Hash {
    async fn execute(&self, mut ctx: Context<'_>) -> Result<ExecResult> {
        let mut names: Vec<String> = Vec::new();
        let (mut clear, mut delete, mut show, mut list) = (false, false, false, false);
        let mut set_path: Option<&str> = None;
        let mut i = 0;
        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            i += 1;
            if !names.is_empty() || arg == "-" || !arg.starts_with('-') {
                names.push(arg.clone());
                continue;
            }
            if arg == "--" {
                names.extend(ctx.args[i..].iter().cloned());
                break;
            }
            for (pos, c) in arg[1..].char_indices() {
                match c {
                    'r' => clear = true,
                    'd' => delete = true,
                    't' => show = true,
                    'l' => list = true,
                    'p' => {
                        let rest = &arg[1 + pos + 1..];
                        if !rest.is_empty() {
                            set_path = Some(rest);
                        } else if i < ctx.args.len() {
                            set_path = Some(ctx.args[i].as_str());
                            i += 1;
                        } else {
                            return Ok(ExecResult::err(
                                format!(
                                    "bash: hash: -p: option requires an argument\n{HASH_USAGE}"
                                ),
                                2,
                            ));
                        }
                        break;
                    }
                    _ => {
                        return Ok(ExecResult::err(
                            format!("bash: hash: -{c}: invalid option\n{HASH_USAGE}"),
                            2,
                        ));
                    }
                }
            }
        }

        let Some(shell) = ctx.shell.take() else {
            return Ok(ExecResult::ok(String::new()));
        };

        if clear {
            shell.command_hash.clear();
        }
        let mut out = String::new();
        let mut err = String::new();
        let mut status = 0;

        if names.is_empty() {
            if clear || set_path.is_some() {
                return Ok(ExecResult::ok(String::new()));
            }
            if delete || show {
                return Ok(ExecResult::err(
                    format!(
                        "bash: hash: -{}: option requires an argument\n",
                        if delete { 'd' } else { 't' }
                    ),
                    1,
                ));
            }
            let entries = shell.command_hash.entries().to_vec();
            let mut lines = String::new();
            for entry in &entries {
                let Some(path) = hashed_path(&ctx, entry).await else {
                    continue;
                };
                if list {
                    lines.push_str(&format!("builtin hash -p {path} {}\n", entry.name));
                } else {
                    lines.push_str(&format!("{:>4}\t{path}\n", entry.hits));
                }
            }
            if lines.is_empty() {
                return Ok(ExecResult::ok("hash: hash table empty\n".to_string()));
            }
            if !list {
                out.push_str("hits\tcommand\n");
            }
            out.push_str(&lines);
            return Ok(ExecResult::ok(out));
        }

        let many = names.len() > 1;
        for name in &names {
            if let Some(path) = &set_path {
                shell
                    .command_hash
                    .insert(name, Some(path), 0, shell.execution_budget)?;
                continue;
            }
            if delete {
                if !shell.command_hash.remove(name) {
                    err.push_str(&format!("bash: hash: {name}: not found\n"));
                    status = 1;
                }
                continue;
            }
            if show || list {
                match shell.command_hash.get(name).cloned() {
                    Some(entry) => {
                        let path = hashed_path(&ctx, &entry).await.unwrap_or_default();
                        if list {
                            out.push_str(&format!("builtin hash -p {path} {name}\n"));
                        } else if many {
                            out.push_str(&format!("{name}\t{path}\n"));
                        } else {
                            out.push_str(&format!("{path}\n"));
                        }
                    }
                    None => {
                        err.push_str(&format!("bash: hash: {name}: not found\n"));
                        status = 1;
                    }
                }
                continue;
            }
            // `hash NAME`: shell builtins and functions are not hashed.
            if name.contains('/')
                || BASH_BUILTIN_NAMES.contains(&name.as_str())
                || shell.has_function(name)
            {
                continue;
            }
            match search_path(&ctx, name).await {
                Some(path) => {
                    let path = if shell.has_builtin(name) {
                        None
                    } else {
                        Some(path)
                    };
                    shell
                        .command_hash
                        .insert(name, path.as_deref(), 0, shell.execution_budget)?;
                }
                None if shell.has_builtin(name) => {}
                None => {
                    err.push_str(&format!("bash: hash: {name}: not found\n"));
                    status = 1;
                }
            }
        }
        Ok(ExecResult {
            stdout: out.into(),
            stderr: err.into(),
            exit_code: status,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::ExecutionLimits;
    use std::sync::atomic::AtomicBool;

    fn budget(bytes: usize) -> ExecutionBudget {
        ExecutionBudget::new(
            &ExecutionLimits::new().max_live_intermediate_bytes(bytes as u64),
            Arc::new(AtomicBool::new(false)),
        )
    }

    #[test]
    fn hash_entry_admission_is_atomic_and_counts_names() {
        let mut names = CommandHash::default();
        assert!(
            names
                .insert(&"n".repeat(256), None, 0, &budget(128))
                .is_err()
        );
        assert!(names.is_empty());

        let path = "p".repeat(64);
        let name = "n".repeat(64);
        let bytes = CommandHash::entry_bytes(&name, Some(&path));
        let budget = budget(bytes);
        let mut table = CommandHash::default();
        table.insert(&name, Some(&path), 0, &budget).unwrap();
        assert!(
            table
                .insert(&name, Some("replacement"), 0, &budget)
                .is_err()
        );
        assert_eq!(
            table.get(&name).unwrap().path.as_deref(),
            Some(path.as_str())
        );
        assert_eq!(table.entries().len(), 1);
    }

    #[test]
    fn hash_entry_removal_replacement_and_eviction_release_leases() {
        let bytes = CommandHash::entry_bytes("name", Some("/path"));
        let budget = budget(bytes * (CommandHash::MAX_ENTRIES + 1));
        let mut table = CommandHash::default();
        for i in 0..1_024 {
            table
                .insert(&format!("{i:04}"), Some("/path"), 0, &budget)
                .unwrap();
        }
        assert_eq!(table.entries().len(), CommandHash::MAX_ENTRIES);
        table.insert("1023", Some("/path"), 0, &budget).unwrap();
        assert!(table.remove("1023"));
        // Exactly one slot was released, including its name and path.
        let slot = budget.lease_bytes(bytes * 2).unwrap();
        drop(slot);
        table.clear();
        budget
            .lease_bytes(bytes * (CommandHash::MAX_ENTRIES + 1))
            .unwrap();
    }

    #[test]
    fn hash_budget_rebinding_preserves_state_on_failure() {
        let bytes = CommandHash::entry_bytes("n", Some("/path"));
        let mut table = CommandHash::default();
        let initial = budget(bytes);
        table.insert("n", Some("/path"), 0, &initial).unwrap();
        assert!(table.bind_budget(&budget(bytes - 1)).is_err());
        assert_eq!(table.get("n").unwrap().path.as_deref(), Some("/path"));
        let next = budget(bytes);
        table.bind_budget(&next).unwrap();
        table.clear();
        next.lease_bytes(bytes).unwrap();
    }

    #[test]
    fn hash_forks_share_payload_leases_and_keep_hit_counts_independent() {
        let bytes = CommandHash::entry_bytes("n", Some("/path"));
        let budget = budget(bytes);
        let mut table = CommandHash::default();
        table.insert("n", Some("/path"), 0, &budget).unwrap();
        let mut fork = table.clone();
        fork.hit("n", None, &budget).unwrap();
        assert_eq!(table.get("n").unwrap().hits, 0);
        assert_eq!(fork.get("n").unwrap().hits, 1);
        assert!(Arc::ptr_eq(
            table.get("n").unwrap().path.as_ref().unwrap(),
            fork.get("n").unwrap().path.as_ref().unwrap(),
        ));
        table.clear();
        fork.clear();
        budget.lease_bytes(bytes).unwrap();
    }
}
