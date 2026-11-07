//! Large-tree stress tests: tree walkers over hundreds of thousands of files
//! must hold bounded memory, list each directory once, and finish.
//!
//! Decisions:
//! - Own test binary: the counting `#[global_allocator]` is process-wide, so
//!   unrelated tests in a shared binary would land in the measurement.
//! - The tree is procedural (`SyntheticFs`): names, sizes and contents are
//!   computed from the path, so the tree itself costs no memory and a peak
//!   measured during a command is the command's own. A real in-memory tree of
//!   this size would dwarf the walkers' allocations.
//! - Default scale (~200k files) keeps the debug-build run in CI to seconds;
//!   `BASHKIT_STRESS_FILES=2000000 cargo test --release --test
//!   large_tree_stress_tests` (`just stress`) runs the same assertions at millions of files.
//! - Memory bound: loading every listing at once costs ~90 B per entry, so
//!   200k files would take ~18 MB and 2M ~180 MB. Walkers must stay under a
//!   fixed `BOUNDED_PEAK` whatever the scale; commands whose output is the
//!   tree (`ls -R`) may add their output size and nothing per entry beyond it.

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bashkit::{
    Bash, DirEntry, ExecutionLimits, FileSystem, FileSystemExt, FileType, Metadata, Result,
};

/// Tests share the process-wide allocator counters, so they run one at a time.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// Tracks live heap bytes and their high-water mark.
struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            if new_size >= layout.size() {
                let grow = new_size - layout.size();
                let live = LIVE.fetch_add(grow, Ordering::Relaxed) + grow;
                PEAK.fetch_max(live, Ordering::Relaxed);
            } else {
                LIVE.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        new
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Peak heap growth any walker may reach, at any scale.
const BOUNDED_PEAK: usize = 8 << 20;

/// Files per leaf directory, leaf directories per top directory.
const FILES: usize = 100;
const SUB: usize = 20;
/// Subdirectories inside the hidden `.git` at the root (each with `FILES`).
const HIDDEN_DIRS: usize = 200;

/// Procedural tree: `/dA/sB/fC.txt` plus `/.git/objects/oH/fC`.
struct SyntheticFs {
    top: usize,
    read_dir: AtomicU64,
    read_file: AtomicU64,
    metadata: AtomicU64,
}

#[derive(Clone, Copy)]
enum Node {
    Root,
    Top(usize),
    Leaf(usize, usize),
    File(usize, usize, usize),
    Git,
    Objects,
    HiddenLeaf,
    HiddenFile,
}

fn index(name: &str, prefix: char, max: usize) -> Option<usize> {
    let n: usize = name.strip_prefix(prefix)?.parse().ok()?;
    (n < max).then_some(n)
}

impl SyntheticFs {
    fn new(files: usize) -> Self {
        Self {
            top: files.div_ceil(SUB * FILES).max(1),
            read_dir: AtomicU64::new(0),
            read_file: AtomicU64::new(0),
            metadata: AtomicU64::new(0),
        }
    }

    fn files(&self) -> usize {
        self.top * SUB * FILES
    }

    /// Directories under the root (not counting it), outside `.git`.
    fn dirs(&self) -> usize {
        self.top + self.top * SUB
    }

    fn reset(&self) {
        for c in [&self.read_dir, &self.read_file, &self.metadata] {
            c.store(0, Ordering::Relaxed);
        }
    }

    fn node(&self, path: &Path) -> Option<Node> {
        let mut node = Node::Root;
        for part in path.components() {
            let name = match part {
                std::path::Component::RootDir => continue,
                std::path::Component::Normal(n) => n.to_str()?,
                _ => return None,
            };
            node = match node {
                Node::Root if name == ".git" => Node::Git,
                Node::Root => Node::Top(index(name, 'd', self.top)?),
                Node::Top(a) => Node::Leaf(a, index(name, 's', SUB)?),
                Node::Leaf(a, b) => {
                    Node::File(a, b, index(name.strip_suffix(".txt")?, 'f', FILES)?)
                }
                Node::Git if name == "objects" => Node::Objects,
                Node::Objects => {
                    index(name, 'o', HIDDEN_DIRS)?;
                    Node::HiddenLeaf
                }
                Node::HiddenLeaf => {
                    index(name, 'f', FILES)?;
                    Node::HiddenFile
                }
                _ => return None,
            };
        }
        Some(node)
    }

    fn content(node: Node) -> Vec<u8> {
        match node {
            Node::File(a, b, c) => format!("line one\nfile {a} {b} {c}\n").into_bytes(),
            _ => b"blob\n".to_vec(),
        }
    }

    fn meta(node: Node) -> Metadata {
        match node {
            Node::File(..) | Node::HiddenFile => Metadata {
                file_type: FileType::File,
                size: Self::content(node).len() as u64,
                mode: 0o644,
                ..Metadata::default()
            },
            _ => Metadata {
                file_type: FileType::Directory,
                mode: 0o755,
                ..Metadata::default()
            },
        }
    }

    fn children(&self, node: Node) -> Option<Vec<(String, Node)>> {
        Some(match node {
            Node::Root => std::iter::once((".git".to_string(), Node::Git))
                .chain((0..self.top).map(|a| (format!("d{a}"), Node::Top(a))))
                .collect(),
            Node::Top(a) => (0..SUB)
                .map(|b| (format!("s{b}"), Node::Leaf(a, b)))
                .collect(),
            Node::Leaf(a, b) => (0..FILES)
                .map(|c| (format!("f{c}.txt"), Node::File(a, b, c)))
                .collect(),
            Node::Git => vec![("objects".to_string(), Node::Objects)],
            Node::Objects => (0..HIDDEN_DIRS)
                .map(|h| (format!("o{h}"), Node::HiddenLeaf))
                .collect(),
            Node::HiddenLeaf => (0..FILES)
                .map(|c| (format!("f{c}"), Node::HiddenFile))
                .collect(),
            Node::File(..) | Node::HiddenFile => return None,
        })
    }
}

fn not_found() -> bashkit::Error {
    std::io::Error::from(std::io::ErrorKind::NotFound).into()
}

fn read_only() -> bashkit::Error {
    std::io::Error::from(std::io::ErrorKind::PermissionDenied).into()
}

#[async_trait]
impl FileSystemExt for SyntheticFs {}

#[async_trait]
impl FileSystem for SyntheticFs {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        self.read_file.fetch_add(1, Ordering::Relaxed);
        match self.node(path) {
            Some(n @ (Node::File(..) | Node::HiddenFile)) => Ok(Self::content(n)),
            Some(_) => Err(std::io::Error::from(std::io::ErrorKind::IsADirectory).into()),
            None => Err(not_found()),
        }
    }
    async fn write_file(&self, _: &Path, _: &[u8]) -> Result<()> {
        Err(read_only())
    }
    async fn append_file(&self, _: &Path, _: &[u8]) -> Result<()> {
        Err(read_only())
    }
    async fn mkdir(&self, _: &Path, _: bool) -> Result<()> {
        Err(read_only())
    }
    async fn remove(&self, _: &Path, _: bool) -> Result<()> {
        Err(read_only())
    }
    async fn stat(&self, path: &Path) -> Result<Metadata> {
        self.metadata.fetch_add(1, Ordering::Relaxed);
        self.node(path).map(Self::meta).ok_or_else(not_found)
    }
    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        self.read_dir.fetch_add(1, Ordering::Relaxed);
        let node = self.node(path).ok_or_else(not_found)?;
        let kids = self.children(node).ok_or_else(|| {
            bashkit::Error::from(std::io::Error::from(std::io::ErrorKind::NotADirectory))
        })?;
        Ok(kids
            .into_iter()
            .map(|(name, n)| DirEntry {
                name,
                metadata: Self::meta(n),
            })
            .collect())
    }
    async fn exists(&self, path: &Path) -> Result<bool> {
        self.metadata.fetch_add(1, Ordering::Relaxed);
        Ok(self.node(path).is_some())
    }
    async fn rename(&self, _: &Path, _: &Path) -> Result<()> {
        Err(read_only())
    }
    async fn copy(&self, _: &Path, _: &Path) -> Result<()> {
        Err(read_only())
    }
    async fn symlink(&self, _: &Path, _: &Path) -> Result<()> {
        Err(read_only())
    }
    async fn read_link(&self, _: &Path) -> Result<PathBuf> {
        Err(std::io::Error::from(std::io::ErrorKind::InvalidInput).into())
    }
    async fn chmod(&self, _: &Path, _: u32) -> Result<()> {
        Err(read_only())
    }
}

fn scale() -> usize {
    std::env::var("BASHKIT_STRESS_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200_000)
}

struct Run {
    peak: usize,
    elapsed: Duration,
    stdout: String,
    exit_code: i32,
    read_dir: u64,
    read_file: u64,
    metadata: u64,
}

async fn setup() -> (Arc<SyntheticFs>, Bash) {
    let fs = Arc::new(SyntheticFs::new(scale()));
    let limits = ExecutionLimits::new()
        .timeout(Duration::from_secs(600))
        .max_commands(usize::MAX / 2)
        .max_work_units(u64::MAX / 2)
        .max_aggregate_input_bytes(u64::MAX / 2);
    let bash = Bash::builder().limits(limits).cwd("/").build();
    bash.mount("/big", fs.clone()).unwrap();
    (fs, bash)
}

async fn measure(fs: &SyntheticFs, bash: &mut Bash, script: &str) -> Run {
    // Warm up lazy statics and caches on a tiny command.
    bash.exec("ls /big > /dev/null").await.unwrap();
    fs.reset();
    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let start = Instant::now();
    let r = bash.exec(script).await.unwrap();
    let elapsed = start.elapsed();
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(before);
    let run = Run {
        peak,
        elapsed,
        stdout: r.stdout.to_string(),
        exit_code: r.exit_code,
        read_dir: fs.read_dir.load(Ordering::Relaxed),
        read_file: fs.read_file.load(Ordering::Relaxed),
        metadata: fs.metadata.load(Ordering::Relaxed),
    };
    eprintln!(
        "{script:44} files={} peak={:.1} MiB time={:.2?} read_dir={} read_file={} metadata={} exit={}",
        fs.files(),
        run.peak as f64 / (1 << 20) as f64,
        run.elapsed,
        run.read_dir,
        run.read_file,
        run.metadata,
        run.exit_code
    );
    run
}

/// Directory walkers with tiny output: bounded memory, one listing per
/// directory, hidden `.git` skipped where the command skips it.
#[tokio::test(flavor = "current_thread")]
async fn walkers_hold_bounded_memory_over_a_huge_tree() {
    let _serial = SERIAL.lock().await;
    let (fs, mut bash) = setup().await;
    let all_dirs = (fs.dirs() + 2 + HIDDEN_DIRS + 1) as u64;
    for (script, out, read_dirs) in [
        ("find /big -name nomatch", "", all_dirs),
        ("find /big -type f -name f99.txt | wc -l", "", all_dirs),
        ("du -s /big", "", all_dirs),
        ("tree -L 2 /big | tail -n 1", "", 1 + fs.top as u64),
        ("grep -rl nomatch /big", "", all_dirs),
    ] {
        let r = measure(&fs, &mut bash, script).await;
        assert!(r.exit_code <= 1, "{script}: exit {}", r.exit_code);
        assert!(r.stdout.contains(out), "{script}");
        assert!(
            r.peak < BOUNDED_PEAK,
            "{script}: peak {} B over {} files (bound {BOUNDED_PEAK} B)",
            r.peak,
            fs.files()
        );
        assert!(r.read_dir <= read_dirs, "{script}: {} read_dir", r.read_dir);
        assert!(r.metadata <= 4, "{script}: {} metadata lookups", r.metadata);
    }
}

/// `ls -R` prints the tree, so its memory is its output, which stops at the
/// stdout limit (1 MiB by default); beyond that it must not hold the tree,
/// and it never enters `.git`.
#[tokio::test(flavor = "current_thread")]
async fn ls_recursive_memory_tracks_output_only() {
    let _serial = SERIAL.lock().await;
    let (fs, mut bash) = setup().await;
    let r = measure(&fs, &mut bash, "ls -R /big").await;
    assert_eq!(r.exit_code, 0);
    // Generation stops at the output limit; prefetch reads at most one
    // walk batch past it, and never `.git`.
    assert!(r.read_dir <= (fs.dirs() + 1) as u64, "entered .git");
    let out = r.stdout.len();
    assert!(out <= 1 << 20, "{out} B of output");
    assert!(
        r.peak < BOUNDED_PEAK + 3 * out,
        "peak {} B for {out} B of output",
        r.peak
    );
}

/// rg must stop at the aggregate-input budget instead of reading the whole
/// tree into memory first.
#[tokio::test(flavor = "current_thread")]
async fn rg_stops_at_the_input_budget_on_a_huge_tree() {
    let _serial = SERIAL.lock().await;
    let fs = Arc::new(SyntheticFs::new(scale()));
    let budget = 1 << 20;
    let limits = ExecutionLimits::new()
        .timeout(Duration::from_secs(600))
        .max_aggregate_input_bytes(budget);
    let mut bash = Bash::builder().limits(limits).cwd("/").build();
    bash.mount("/big", fs.clone()).unwrap();
    fs.reset();
    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let r = bash.exec("rg -l nomatch /big").await;
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(before);
    let reads = fs.read_file.load(Ordering::Relaxed);
    eprintln!(
        "rg over budget: peak={peak} B reads={reads} result_ok={}",
        r.is_ok()
    );
    assert!(r.is_err(), "expected the input budget to stop rg");
    // ~25 B per file: the 1 MiB budget covers ~40k files of the tree.
    assert!((reads as usize) < fs.files() / 2, "read {reads} files");
    // Each file is searched and dropped as it is read, so nothing scales
    // with reads: ~5 MiB here. Holding the read text took ~13.7 MB (17 MB
    // under tarpaulin); the old whole-tree candidate list ~70 MB.
    assert!(peak < 2 * BOUNDED_PEAK, "peak {peak} B for {reads} reads");
}

/// rg within budget over the whole tree: each file is searched as it is
/// read and dropped, so memory stays bounded however many files it searches
/// (it held ~160 B per file, ~300 MB at 2M files, before searching moved
/// into the walk). Several roots stream one after another the same way.
#[tokio::test(flavor = "current_thread")]
async fn rg_within_budget_holds_bounded_memory() {
    let _serial = SERIAL.lock().await;
    let (fs, mut bash) = setup().await;
    // The first rg in the process builds one-time state (4 MiB locally, more
    // on CI, where it pushed this past BOUNDED_PEAK). It is not per-file
    // memory, so pay it on a small subtree first and measure only the walk.
    bash.exec("rg -l nomatch /big/d0 > /dev/null")
        .await
        .unwrap();
    for cmd in [
        "rg -l nomatch /big",
        "rg -c nomatch /big/d1 /big/d0",
        "rg nomatch /big",
    ] {
        let r = measure(&fs, &mut bash, cmd).await;
        assert_eq!(r.exit_code, 1, "{cmd}");
        assert!(r.peak < BOUNDED_PEAK, "{cmd}: peak {} B", r.peak);
    }
    let r = measure(&fs, &mut bash, "rg -l nomatch /big").await;
    // Every file, plus any ignore file the listing shows.
    assert!(r.read_file as usize >= fs.files());
}
