//! find builtin - search a directory hierarchy with GNU find's expression
//! language.
//!
//! Decisions:
//! - Full GNU grammar (`( )`, `!`, `-a`, `-o`, `,`) parsed into an [`Expr`]
//!   tree (`parse.rs`); error texts follow findutils 4.9 in the C locale.
//! - Evaluation runs in a resumable driver ([`FindRun`]). `-exec ... \;` is a
//!   real predicate: the driver yields the command to the interpreter
//!   ([`ExecutionPlan::Driver`]) and resumes with its exit status. Without
//!   `-exec`/`-execdir`, `execute()` drives the same machine to completion.
//! - Per-entry evaluation records its effects (output, exec output, delete
//!   messages) and commits them only once the whole expression finished for
//!   that entry. On suspension the effects are discarded and the expression
//!   re-evaluated with the cached exec/delete results, so output order matches
//!   GNU and side effects run exactly once.
//! - Directory entries are visited in byte-sorted order (deterministic; GNU
//!   uses readdir order).
//! - Time tests read the shared virtual clock (TM-INF-018); ownership tests
//!   see the single virtual user (uid/gid 1000). The VFS has no atime/ctime,
//!   so those read the modification time.
//! - THREAT[TM-DOS-121]: `-L` loop detection, budget-charged listings, capped
//!   output, bounded `{} +` batches.
//! - `-ok`/`-okdir`, `-ls`, `-fprint*`, `-samefile`, `-inum`, `-links`,
//!   `-fstype`, `-context` are not implemented (L-FIND-001).

mod parse;
mod printf;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;

use parse::{Cmp, Expr, Follow, Identity, NewerRef, Options, PermKind, TimeField};

use super::limits::FIND_MAX_OUTPUT_BYTES;
use super::{
    Builtin, Context, Date, ExecutionPlan, PlanDriver, PlanStep, SubCommand, fnmatch, resolve_path,
};
use crate::StreamData;
use crate::error::Result;
use crate::fs::{FileSystem, Metadata, vfs_join};
use crate::interpreter::ExecResult;

const HELP: &str = "Usage: find [-H] [-L] [-P] [PATH...] [EXPRESSION]\nSearch for files in a directory hierarchy.\n\nOperators: ( EXPR ), ! EXPR, -not EXPR, EXPR -a EXPR, EXPR -o EXPR, EXPR , EXPR\nOptions: -maxdepth N -mindepth N -depth -follow -daystart -regextype TYPE -xdev\nTests: -name -iname -path -ipath -lname -regex -iregex -type -xtype -size -empty\n       -mtime -mmin -atime -amin -ctime -cmin -newer -newerXY -perm -readable\n       -writable -executable -user -group -uid -gid -nouser -nogroup -true -false\nActions: -print -print0 -printf FMT -delete -prune -quit\n         -exec CMD {} ; -exec CMD {} + -execdir CMD {} ; -execdir CMD {} +\n      --help\tdisplay this help and exit\n      --version\toutput version information and exit\n";

/// Symlink hops before ELOOP, matching Linux.
const MAX_SYMLINK_HOPS: usize = 40;
/// Paths passed per `-exec ... {} +` invocation before a new one starts.
const EXEC_BATCH_MAX: usize = 4096;
const OUTPUT_CAP_MSG: &str = "find: output size limit exceeded\n";
const VIRTUAL_UID: u64 = super::system::SANDBOX_UID as u64;
const VIRTUAL_GID: u64 = super::system::SANDBOX_GID as u64;

/// The find builtin.
pub struct Find {
    clock: Date,
    username: String,
}

impl Default for Find {
    fn default() -> Self {
        Self::new(Date::default(), super::DEFAULT_USERNAME)
    }
}

impl Find {
    /// Create a find builtin reading time from `clock` and owned by `username`.
    pub fn new(clock: Date, username: impl Into<String>) -> Self {
        Self {
            clock,
            username: username.into(),
        }
    }

    async fn build(&self, ctx: &Context<'_>) -> std::result::Result<FindRun, String> {
        let ident = Identity {
            username: &self.username,
            uid: VIRTUAL_UID,
            gid: VIRTUAL_GID,
        };
        let parsed = parse::parse(ctx.args, &ident)?;
        FindRun::new(parsed, ctx, self.clock, &self.username).await
    }
}

/// Epoch nanoseconds of a VFS timestamp (0 when before the epoch).
pub(super) fn nanos_of(t: crate::time_compat::SystemTime) -> i128 {
    t.duration_since(crate::time_compat::UNIX_EPOCH)
        .map(|d| d.as_nanos() as i128)
        .unwrap_or(0)
}

/// Formatting context for `-printf` time and owner directives.
pub(super) struct FmtEnv {
    clock: Date,
    tz: Option<String>,
    pub(super) username: String,
}

impl FmtEnv {
    pub(super) fn strftime(&self, secs: i64, fmt: &str) -> String {
        self.clock
            .strftime(self.tz.as_ref(), Some(secs), fmt)
            .unwrap_or_default()
    }
}

/// One visited file.
pub(super) struct Entry {
    /// Absolute path of the entry itself (not following a final symlink).
    abs: PathBuf,
    /// Directory to list when descending (canonical when following links).
    dir_path: PathBuf,
    pub(super) display: String,
    root: usize,
    pub(super) depth: usize,
    pub(super) lmeta: Metadata,
    /// Metadata of the symlink target (None: not a symlink, or dangling).
    pub(super) fmeta: Option<Metadata>,
    pub(super) link_target: Option<String>,
    follow: bool,
    subdirs: usize,
}

impl Entry {
    pub(super) fn meta(&self) -> &Metadata {
        if self.follow {
            self.fmeta.as_ref().unwrap_or(&self.lmeta)
        } else {
            &self.lmeta
        }
    }

    /// The metadata `-xtype` reads (the opposite follow mode).
    fn xmeta(&self) -> &Metadata {
        if self.follow {
            &self.lmeta
        } else {
            self.fmeta.as_ref().unwrap_or(&self.lmeta)
        }
    }

    pub(super) fn nlink(&self) -> usize {
        if self.meta().file_type.is_dir() {
            2 + self.subdirs
        } else {
            1
        }
    }

    /// Name tested by `-name`: last component, trailing slashes ignored.
    fn name(&self) -> &str {
        let trimmed = self.display.trim_end_matches('/');
        if trimmed.is_empty() {
            return "/";
        }
        trimmed.rsplit('/').next().unwrap_or(trimmed)
    }
}

fn type_matches(meta: &Metadata, types: &[char]) -> bool {
    let c = if meta.file_type.is_dir() {
        'd'
    } else if meta.file_type.is_symlink() {
        'l'
    } else if matches!(meta.file_type, crate::fs::FileType::Fifo) {
        'p'
    } else {
        'f'
    };
    types.contains(&c)
}

/// Resolve every symlink in `path` (realpath), with an ELOOP hop cap.
async fn canonicalize(fs: &dyn FileSystem, path: &Path) -> std::result::Result<PathBuf, String> {
    let mut rest: VecDeque<String> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            std::path::Component::ParentDir => Some("..".to_string()),
            _ => None,
        })
        .collect();
    let mut out = PathBuf::from("/");
    let mut hops = 0;
    while let Some(comp) = rest.pop_front() {
        if comp == ".." {
            out.pop();
            continue;
        }
        let candidate = out.join(&comp);
        match fs.lstat(&candidate).await {
            Ok(m) if m.file_type.is_symlink() => {
                hops += 1;
                if hops > MAX_SYMLINK_HOPS {
                    return Err("Too many levels of symbolic links".to_string());
                }
                let target = fs.read_link(&candidate).await.map_err(|e| e.to_string())?;
                if target.is_absolute() {
                    out = PathBuf::from("/");
                }
                let mut parts: Vec<String> = target
                    .components()
                    .filter_map(|c| match c {
                        std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                        std::path::Component::ParentDir => Some("..".to_string()),
                        _ => None,
                    })
                    .collect();
                while let Some(p) = parts.pop() {
                    rest.push_front(p);
                }
            }
            Ok(_) => out = candidate,
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(out)
}

/// A directory on the current path, for `-L` loop detection.
struct Ancestor {
    canon: PathBuf,
    display: String,
    parent: Option<Arc<Ancestor>>,
}

enum Work {
    Visit {
        abs: PathBuf,
        display: String,
        root: usize,
        depth: usize,
        ancestors: Option<Arc<Ancestor>>,
    },
    /// `-depth`: evaluate a directory after its children.
    Post(Box<Entry>),
}

enum Cached {
    Exec {
        ok: bool,
        out: StreamData,
        err: StreamData,
    },
    Delete {
        ok: bool,
        msg: Option<String>,
    },
}

struct Pending {
    entry: Entry,
    post: bool,
    cache: HashMap<usize, Cached>,
    ancestors: Option<Arc<Ancestor>>,
}

enum Effect {
    Out(String),
    ExecOut(usize),
    DeleteMsg(usize),
    Batch {
        id: usize,
        path: String,
        dir: Option<PathBuf>,
    },
}

enum Halt {
    Suspend {
        id: usize,
        command: SubCommand,
        cwd: Option<PathBuf>,
    },
    Quit,
}

enum Awaiting {
    Exec(usize),
    Batch,
}

struct Batch {
    id: usize,
    dir: Option<PathBuf>,
    paths: Vec<String>,
}

/// Read-only state shared by every evaluation.
struct RunCfg {
    expr: Expr,
    opts: Options,
    roots: Vec<String>,
    fs: Arc<dyn FileSystem>,
    now: i128,
    fmt: FmtEnv,
}

struct Cx<'a> {
    cfg: &'a RunCfg,
    entry: &'a Entry,
    cache: &'a mut HashMap<usize, Cached>,
    effects: Vec<Effect>,
    prune: bool,
}

type EvalFuture<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = std::result::Result<bool, Halt>> + Send + 'a>,
>;

fn exec_path(entry: &Entry, dir: bool) -> String {
    if dir {
        format!("./{}", entry.name())
    } else {
        entry.display.clone()
    }
}

async fn delete_entry(fs: &dyn FileSystem, entry: &Entry) -> Cached {
    // GNU silently skips the `.` starting point.
    if entry.depth == 0 && matches!(entry.display.as_str(), "." | "./") {
        return Cached::Delete {
            ok: true,
            msg: None,
        };
    }
    let result = if entry.lmeta.file_type.is_dir() {
        match fs.read_dir(&entry.abs).await {
            Ok(children) if !children.is_empty() => Err("Directory not empty".to_string()),
            Ok(_) => fs
                .remove(&entry.abs, false)
                .await
                .map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        }
    } else {
        fs.remove(&entry.abs, false)
            .await
            .map_err(|e| e.to_string())
    };
    match result {
        Ok(()) => Cached::Delete {
            ok: true,
            msg: None,
        },
        Err(e) => Cached::Delete {
            ok: false,
            msg: Some(format!("find: cannot delete '{}': {e}\n", entry.display)),
        },
    }
}

fn eval<'a, 'b: 'a>(e: &'a Expr, cx: &'a mut Cx<'b>) -> EvalFuture<'a> {
    Box::pin(async move {
        let entry = cx.entry;
        let meta = entry.meta();
        Ok(match e {
            Expr::And(a, b) => eval(a, cx).await? && eval(b, cx).await?,
            Expr::Or(a, b) => eval(a, cx).await? || eval(b, cx).await?,
            Expr::Comma(a, b) => {
                eval(a, cx).await?;
                eval(b, cx).await?
            }
            Expr::Not(a) => !eval(a, cx).await?,
            Expr::Const(v) => *v,
            Expr::Name { pat, nocase } => fnmatch(entry.name(), pat, *nocase),
            Expr::Path { pat, nocase } => fnmatch(&entry.display, pat, *nocase),
            Expr::Lname { pat, nocase } => entry
                .link_target
                .as_deref()
                .is_some_and(|t| fnmatch(t, pat, *nocase)),
            Expr::Regex(re) => re.is_match(&entry.display),
            Expr::Type { types, xtype } => {
                type_matches(if *xtype { entry.xmeta() } else { meta }, types)
            }
            Expr::Size { cmp, n, unit } => cmp.test(meta.size.div_ceil(*unit), *n),
            Expr::Empty => {
                if meta.file_type.is_dir() {
                    cx.cfg
                        .fs
                        .read_dir(&entry.dir_path)
                        .await
                        .is_ok_and(|c| c.is_empty())
                } else {
                    meta.file_type.is_file() && meta.size == 0
                }
            }
            Expr::Age {
                field,
                minutes,
                cmp,
                n,
            } => {
                let t = match field {
                    TimeField::Modified => nanos_of(meta.modified),
                    TimeField::Created => nanos_of(meta.created),
                };
                let age = cx.cfg.now - t;
                let n = i128::from(*n);
                if *minutes {
                    let unit = 60_000_000_000i128;
                    match cmp {
                        Cmp::Gt => age > n * unit,
                        Cmp::Lt => age < n * unit,
                        Cmp::Eq => age > (n - 1) * unit && age <= n * unit,
                    }
                } else {
                    cmp.test(age.div_euclid(86_400_000_000_000), n)
                }
            }
            Expr::Newer { field, reference } => {
                let t = match field {
                    TimeField::Modified => nanos_of(meta.modified),
                    TimeField::Created => nanos_of(meta.created),
                };
                match reference {
                    NewerRef::Nanos(r) => t > *r,
                    // Resolved before traversal.
                    NewerRef::File(_) | NewerRef::Date(_) => false,
                }
            }
            Expr::Perm { mode, kind } => {
                let m = meta.mode & 0o7777;
                match kind {
                    PermKind::Exact => m == *mode,
                    PermKind::All => m & mode == *mode,
                    PermKind::Any => *mode == 0 || m & mode != 0,
                }
            }
            Expr::Access(bit) => (meta.mode >> 6) & bit != 0,
            Expr::Uid { cmp, n } => cmp.test(VIRTUAL_UID, *n),
            Expr::Gid { cmp, n } => cmp.test(VIRTUAL_GID, *n),
            Expr::Print { nul } => {
                let term = if *nul { '\0' } else { '\n' };
                cx.effects
                    .push(Effect::Out(format!("{}{term}", entry.display)));
                true
            }
            Expr::Printf(fmt) => {
                let start = &cx.cfg.roots[entry.root];
                cx.effects
                    .push(Effect::Out(printf::render(fmt, entry, start, &cx.cfg.fmt)));
                true
            }
            Expr::Delete { id } => {
                if !cx.cache.contains_key(id) {
                    let done = delete_entry(cx.cfg.fs.as_ref(), entry).await;
                    cx.cache.insert(*id, done);
                }
                match cx.cache.get(id) {
                    Some(Cached::Delete { ok, msg }) => {
                        if msg.is_some() {
                            cx.effects.push(Effect::DeleteMsg(*id));
                        }
                        *ok
                    }
                    _ => false,
                }
            }
            Expr::Prune => {
                cx.prune = true;
                true
            }
            Expr::Quit => return Err(Halt::Quit),
            Expr::Exec {
                id,
                argv,
                batch,
                dir,
            } => {
                let path = exec_path(entry, *dir);
                let cwd = dir.then(|| {
                    entry
                        .abs
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| PathBuf::from("/"))
                });
                if *batch {
                    cx.effects.push(Effect::Batch {
                        id: *id,
                        path,
                        dir: cwd,
                    });
                    true
                } else if let Some(Cached::Exec { ok, .. }) = cx.cache.get(id) {
                    cx.effects.push(Effect::ExecOut(*id));
                    *ok
                } else {
                    let args: Vec<String> = argv.iter().map(|a| a.replace("{}", &path)).collect();
                    return Err(Halt::Suspend {
                        id: *id,
                        command: SubCommand {
                            name: args[0].clone(),
                            args: args[1..].to_vec(),
                            stdin: None,
                            assignments: Vec::new(),
                        },
                        cwd,
                    });
                }
            }
        })
    })
}

/// Resumable find execution (see module docs).
pub(super) struct FindRun {
    cfg: RunCfg,
    cwd: PathBuf,
    budget: Option<crate::ExecutionCapability<crate::limits::ExecutionBudget>>,
    exec_templates: HashMap<usize, Vec<String>>,
    needs_nlink: bool,
    next_root: usize,
    stack: Vec<Work>,
    current: Option<Pending>,
    awaiting: Option<Awaiting>,
    batches: Vec<Batch>,
    ready: VecDeque<(SubCommand, Option<PathBuf>)>,
    flushed: bool,
    quit: bool,
    stdout: StreamData,
    stderr: StreamData,
    own_bytes: usize,
    output_cap: usize,
    failed: bool,
}

impl FindRun {
    async fn new(
        parsed: parse::Parsed,
        ctx: &Context<'_>,
        clock: Date,
        username: &str,
    ) -> std::result::Result<Self, String> {
        let parse::Parsed {
            paths,
            mut expr,
            opts,
            ..
        } = parsed;
        // Resolve -newer reference files before traversal (GNU fails early).
        let mut refs: Vec<String> = Vec::new();
        expr.walk(&mut |e| {
            if let Expr::Newer {
                reference: NewerRef::File(f),
                ..
            } = e
            {
                refs.push(f.clone());
            }
        });
        let mut dates: Vec<String> = Vec::new();
        expr.walk(&mut |e| {
            if let Expr::Newer {
                reference: NewerRef::Date(d),
                ..
            } = e
            {
                dates.push(d.clone());
            }
        });
        let mut resolved = HashMap::new();
        for d in dates {
            match clock.parse_date(ctx.env.get("TZ"), &d) {
                Ok(dt) => {
                    let nanos = i128::from(dt.timestamp()) * 1_000_000_000
                        + i128::from(dt.timestamp_subsec_nanos());
                    resolved.insert(d, nanos);
                }
                Err(_) => {
                    return Err(format!(
                        "find: I cannot figure out how to interpret `{d}' as a date or time\n"
                    ));
                }
            }
        }
        for f in refs {
            let path = resolve_path(ctx.cwd, &f);
            match ctx.fs.stat(&path).await {
                Ok(m) => {
                    resolved.insert(f, nanos_of(m.modified));
                }
                Err(_) => return Err(format!("find: '{f}': No such file or directory\n")),
            }
        }
        let mut exec_templates = HashMap::new();
        let mut needs_nlink = false;
        expr.walk_mut(&mut |e| match e {
            Expr::Newer { reference, .. } => {
                if let NewerRef::File(key) | NewerRef::Date(key) = reference {
                    *reference = NewerRef::Nanos(resolved.get(key.as_str()).copied().unwrap_or(0));
                }
            }
            Expr::Exec { id, argv, .. } => {
                exec_templates.insert(*id, argv.clone());
            }
            Expr::Printf(f) => needs_nlink |= f.contains("%n"),
            _ => {}
        });
        let (secs, nanos) = clock.now_epoch();
        let mut now = i128::from(secs) * 1_000_000_000 + i128::from(nanos);
        if opts.daystart {
            let day = 86_400_000_000_000i128;
            now = now.div_euclid(day) * day + day;
        }
        Ok(Self {
            cfg: RunCfg {
                expr,
                opts,
                roots: paths,
                fs: ctx.fs.clone(),
                now,
                fmt: FmtEnv {
                    clock,
                    tz: ctx.env.get("TZ").cloned(),
                    username: username.to_string(),
                },
            },
            cwd: ctx.cwd.clone(),
            budget: ctx.execution_budget(),
            exec_templates,
            needs_nlink,
            next_root: 0,
            stack: Vec::new(),
            current: None,
            awaiting: None,
            batches: Vec::new(),
            ready: VecDeque::new(),
            flushed: false,
            quit: false,
            stdout: StreamData::new(),
            stderr: StreamData::new(),
            own_bytes: 0,
            output_cap: FIND_MAX_OUTPUT_BYTES,
            failed: false,
        })
    }

    fn consume_work(&self, units: usize) -> Result<()> {
        if let Some(budget) = &self.budget {
            budget
                .try_with(|b| b.consume_work(u64::try_from(units).unwrap_or(u64::MAX)))
                .map_err(|_| crate::Error::Cancelled)??;
        }
        Ok(())
    }

    fn error(&mut self, msg: String) {
        self.stderr.push_str(&msg);
        self.failed = true;
    }

    fn batch_command(&self, batch: Batch) -> (SubCommand, Option<PathBuf>) {
        let template = self
            .exec_templates
            .get(&batch.id)
            .cloned()
            .unwrap_or_default();
        let mut argv: Vec<String> = template[..template.len().saturating_sub(1)].to_vec();
        argv.extend(batch.paths);
        let name = argv.first().cloned().unwrap_or_default();
        (
            SubCommand {
                name,
                args: argv.into_iter().skip(1).collect(),
                stdin: None,
                assignments: Vec::new(),
            },
            batch.dir,
        )
    }

    fn commit(&mut self, effects: Vec<Effect>, cache: &HashMap<usize, Cached>) {
        for effect in effects {
            match effect {
                Effect::Out(text) => {
                    if self.own_bytes + text.len() > self.output_cap {
                        self.error(OUTPUT_CAP_MSG.to_string());
                        self.quit = true;
                        return;
                    }
                    self.own_bytes += text.len();
                    self.stdout.push_str(&text);
                }
                Effect::ExecOut(id) => {
                    if let Some(Cached::Exec { out, err, .. }) = cache.get(&id) {
                        self.stdout.append(out);
                        self.stderr.append(err);
                    }
                }
                Effect::DeleteMsg(id) => {
                    if let Some(Cached::Delete { msg: Some(m), .. }) = cache.get(&id) {
                        let m = m.clone();
                        self.error(m);
                    }
                }
                Effect::Batch { id, path, dir } => {
                    let idx = match self.batches.iter().position(|b| b.id == id && b.dir == dir) {
                        Some(i) => i,
                        None => {
                            self.batches.push(Batch {
                                id,
                                dir,
                                paths: Vec::new(),
                            });
                            self.batches.len() - 1
                        }
                    };
                    self.batches[idx].paths.push(path);
                    if self.batches[idx].paths.len() >= EXEC_BATCH_MAX {
                        let full = self.batches.remove(idx);
                        let cmd = self.batch_command(full);
                        self.ready.push_back(cmd);
                    }
                }
            }
        }
    }

    async fn start_root(&mut self, index: usize) {
        let root = self.cfg.roots[index].clone();
        let abs = resolve_path(&self.cwd, &root);
        if self.cfg.fs.lstat(&abs).await.is_err() {
            self.error(format!("find: '{root}': No such file or directory\n"));
            return;
        }
        self.stack.push(Work::Visit {
            abs,
            display: root,
            root: index,
            depth: 0,
            ancestors: None,
        });
    }

    fn follows(&self, depth: usize) -> bool {
        match self.cfg.opts.follow {
            Follow::Never => false,
            Follow::Roots => depth == 0,
            Follow::Always => true,
        }
    }

    async fn visit(&mut self, work: Work) -> Result<()> {
        let (abs, display, root, depth, ancestors) = match work {
            Work::Post(entry) => {
                self.current = Some(Pending {
                    entry: *entry,
                    post: true,
                    cache: HashMap::new(),
                    ancestors: None,
                });
                return Ok(());
            }
            Work::Visit {
                abs,
                display,
                root,
                depth,
                ancestors,
            } => (abs, display, root, depth, ancestors),
        };
        let fs = self.cfg.fs.clone();
        let lmeta = match fs.lstat(&abs).await {
            Ok(m) => m,
            Err(e) => {
                self.error(format!("find: '{display}': {e}\n"));
                return Ok(());
            }
        };
        let follow = self.follows(depth);
        let (link_target, fmeta, dir_path) = if lmeta.file_type.is_symlink() {
            let target = fs
                .read_link(&abs)
                .await
                .ok()
                .map(|t| t.to_string_lossy().into_owned());
            match canonicalize(fs.as_ref(), &abs).await {
                Ok(canon) => {
                    let m = fs.stat(&canon).await.ok();
                    (target, m, canon)
                }
                Err(_) => (target, None, abs.clone()),
            }
        } else {
            (None, None, abs.clone())
        };
        let mut entry = Entry {
            abs,
            dir_path,
            display,
            root,
            depth,
            lmeta,
            fmeta,
            link_target,
            follow,
            subdirs: 0,
        };
        let is_dir = entry.meta().file_type.is_dir();
        if is_dir && self.needs_nlink {
            entry.subdirs = fs
                .read_dir(&entry.dir_path)
                .await
                .map(|c| c.iter().filter(|d| d.metadata.file_type.is_dir()).count())
                .unwrap_or(0);
        }
        if is_dir && self.cfg.opts.follow != Follow::Never {
            let mut cur = ancestors.as_deref();
            while let Some(a) = cur {
                if a.canon == entry.dir_path {
                    let msg = format!(
                        "find: File system loop detected; '{}' is part of the same file system loop as '{}'.\n",
                        entry.display, a.display
                    );
                    self.error(msg);
                    return Ok(());
                }
                cur = a.parent.as_deref();
            }
        }
        if self.cfg.opts.depth_first && is_dir {
            let children = self.children(&entry, ancestors).await?;
            self.stack.push(Work::Post(Box::new(entry)));
            self.stack.extend(children.into_iter().rev());
        } else {
            // Pre-order: children are listed after evaluation, so -prune
            // and -quit can stop them.
            self.current = Some(Pending {
                entry,
                post: false,
                cache: HashMap::new(),
                ancestors,
            });
        }
        Ok(())
    }

    /// List `entry`'s children as Visit work items (empty for non-directories
    /// and at -maxdepth).
    async fn children(
        &mut self,
        entry: &Entry,
        ancestors: Option<Arc<Ancestor>>,
    ) -> Result<Vec<Work>> {
        if !entry.meta().file_type.is_dir()
            || self.cfg.opts.max_depth.is_some_and(|m| entry.depth >= m)
        {
            return Ok(Vec::new());
        }
        let mut list = match self.cfg.fs.read_dir(&entry.dir_path).await {
            Ok(list) => list,
            Err(e) => {
                self.error(format!("find: '{}': {e}\n", entry.display));
                return Ok(Vec::new());
            }
        };
        self.consume_work(list.len())?;
        list.sort_by(|a, b| a.name.cmp(&b.name));
        let chain = (self.cfg.opts.follow != Follow::Never).then(|| {
            Arc::new(Ancestor {
                canon: entry.dir_path.clone(),
                display: entry.display.clone(),
                parent: ancestors,
            })
        });
        Ok(list
            .into_iter()
            .map(|child| Work::Visit {
                abs: vfs_join(&entry.dir_path, &child.name),
                display: if entry.display.ends_with('/') {
                    format!("{}{}", entry.display, child.name)
                } else {
                    format!("{}/{}", entry.display, child.name)
                },
                root: entry.root,
                depth: entry.depth + 1,
                ancestors: chain.clone(),
            })
            .collect())
    }

    fn finish(&mut self) -> ExecResult {
        ExecResult {
            stdout: std::mem::take(&mut self.stdout),
            stderr: std::mem::take(&mut self.stderr),
            exit_code: i32::from(self.failed),
            ..Default::default()
        }
    }
}

#[async_trait]
impl PlanDriver for FindRun {
    async fn next(&mut self, last: Option<ExecResult>) -> Result<PlanStep> {
        if let Some(result) = last {
            match self.awaiting.take() {
                Some(Awaiting::Exec(id)) => {
                    if let Some(p) = self.current.as_mut() {
                        p.cache.insert(
                            id,
                            Cached::Exec {
                                ok: result.exit_code == 0,
                                out: result.stdout,
                                err: result.stderr,
                            },
                        );
                    }
                }
                Some(Awaiting::Batch) => {
                    self.stdout.append(&result.stdout);
                    self.stderr.append(&result.stderr);
                    if result.exit_code != 0 {
                        self.failed = true;
                    }
                }
                None => {}
            }
        }
        loop {
            if let Some((command, cwd)) = self.ready.pop_front() {
                self.awaiting = Some(Awaiting::Batch);
                return Ok(PlanStep::Run { command, cwd });
            }
            if let Some(mut pending) = self.current.take() {
                let skip = pending.entry.depth < self.cfg.opts.min_depth;
                let (outcome, effects, prune) = if skip {
                    (Ok(true), Vec::new(), false)
                } else {
                    let mut cx = Cx {
                        cfg: &self.cfg,
                        entry: &pending.entry,
                        cache: &mut pending.cache,
                        effects: Vec::new(),
                        prune: false,
                    };
                    let outcome = eval(&self.cfg.expr, &mut cx).await;
                    (outcome, cx.effects, cx.prune)
                };
                match outcome {
                    Err(Halt::Suspend { id, command, cwd }) => {
                        self.current = Some(pending);
                        self.awaiting = Some(Awaiting::Exec(id));
                        return Ok(PlanStep::Run { command, cwd });
                    }
                    Err(Halt::Quit) => {
                        self.commit(effects, &pending.cache);
                        self.quit = true;
                    }
                    Ok(_) => {
                        self.commit(effects, &pending.cache);
                        if !pending.post && !prune && !self.quit {
                            let children = self.children(&pending.entry, pending.ancestors).await?;
                            self.stack.extend(children.into_iter().rev());
                        }
                    }
                }
                continue;
            }
            if !self.quit {
                if let Some(work) = self.stack.pop() {
                    self.visit(work).await?;
                    continue;
                }
                if self.next_root < self.cfg.roots.len() {
                    let index = self.next_root;
                    self.next_root += 1;
                    self.start_root(index).await;
                    continue;
                }
            }
            if !self.flushed {
                // GNU runs pending `{} +` command lines even after -quit.
                self.flushed = true;
                let batches = std::mem::take(&mut self.batches);
                for batch in batches {
                    let cmd = self.batch_command(batch);
                    self.ready.push_back(cmd);
                }
                continue;
            }
            return Ok(PlanStep::Done(self.finish()));
        }
    }
}

#[async_trait]
impl Builtin for Find {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(ctx.args, HELP, Some("find (bashkit) 0.1")) {
            return Ok(r);
        }
        let mut run = match self.build(&ctx).await {
            Ok(run) => run,
            Err(msg) => return Ok(ExecResult::err(msg, 1)),
        };
        match run.next(None).await? {
            PlanStep::Done(result) => Ok(result),
            // Only reachable if the plan path was skipped; -exec needs it.
            PlanStep::Run { .. } => Ok(ExecResult::err(
                "find: -exec is not available in this context\n".to_string(),
                1,
            )),
        }
    }

    async fn execution_plan(&self, ctx: &Context<'_>) -> Result<Option<ExecutionPlan>> {
        let ident = Identity {
            username: &self.username,
            uid: VIRTUAL_UID,
            gid: VIRTUAL_GID,
        };
        if !parse::parse(ctx.args, &ident).is_ok_and(|p| p.has_exec) {
            return Ok(None);
        }
        match self.build(ctx).await {
            Ok(run) => Ok(Some(ExecutionPlan::Driver(Box::new(run)))),
            // execute() reports the error.
            Err(_) => Ok(None),
        }
    }
}
