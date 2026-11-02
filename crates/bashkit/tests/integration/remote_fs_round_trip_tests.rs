//! Round-trip budgets for tree-walking builtins over a remote-style backend.
//!
//! `CountingFs` counts every `FileSystem` call. On a backend where each call
//! is a network round trip (~10 ms), a builtin that issues a `stat` per
//! listed entry, probes for ignore files that do not exist, or reads every
//! file when an index could answer, times out on a modest checkout. These
//! tests pin each command's call budget on a 1,000-file tree, and check that
//! the cheaper paths (index, listing metadata, bulk `walk`) print exactly what
//! the plain paths print.
//!
//! Set `BASHKIT_ROUND_TRIPS=1` to print every command's counts.

use bashkit::testing::{CallCounts, CountingFs};
use bashkit::{Bash, FileSystem};
use std::path::Path;
use std::sync::Arc;

/// 10 x 10 directories, 10 files each: 1,000 files, 111 directories under
/// `/workspace/tree` (counting `tree` itself).
const TOP: usize = 10;
const SUB: usize = 10;
const FILES: usize = 10;
const DIRS: u64 = 1 + TOP as u64 + (TOP * SUB) as u64;
const TOTAL_FILES: u64 = (TOP * SUB * FILES) as u64;
/// Every 50th file holds the needle: 20 matches.
const NEEDLE_EVERY: usize = 50;
const MATCHES: u64 = (TOP * SUB * FILES / NEEDLE_EVERY) as u64;
/// Directories an index answer may list to apply ignore rules: the two
/// ancestors above the root, the root, and at most one per match level.
const INDEX_LISTING_BUDGET: u64 = 2 + 1 + TOP as u64 + MATCHES;

async fn seed(fs: &dyn FileSystem) {
    let mut n = 0;
    fs.mkdir(Path::new("/workspace/tree"), true).await.unwrap();
    for a in 0..TOP {
        for b in 0..SUB {
            let dir = format!("/workspace/tree/d{a}/s{b}");
            fs.mkdir(Path::new(&dir), true).await.unwrap();
            for c in 0..FILES {
                let body = if n % NEEDLE_EVERY == 0 {
                    format!("line one\nhas needle here\nfile {n}\n")
                } else {
                    format!("line one\nnothing\nfile {n}\n")
                };
                fs.write_file(Path::new(&format!("{dir}/f{c}.txt")), body.as_bytes())
                    .await
                    .unwrap();
                n += 1;
            }
        }
    }
}

async fn setup(fs: CountingFs) -> (Arc<CountingFs>, Bash) {
    let fs = Arc::new(fs);
    seed(fs.as_ref()).await;
    let bash = Bash::builder().fs(fs.clone()).cwd("/workspace").build();
    (fs, bash)
}

async fn run(fs: &CountingFs, bash: &mut Bash, script: &str) -> (CallCounts, String) {
    fs.reset();
    let r = bash.exec(script).await.unwrap();
    let counts = fs.counts();
    if std::env::var("BASHKIT_ROUND_TRIPS").is_ok() {
        println!("{script:48} {counts}\n  stderr: {}", r.stderr);
    }
    assert_eq!(r.exit_code, 0, "{script}: {}", r.stderr);
    (counts, r.stdout.to_string())
}

fn line_count(out: &str) -> u64 {
    out.lines().count() as u64
}

// ---- rg ----------------------------------------------------------------

/// Acceptance: `rg PATTERN dir` with ignore rules on makes one `search`
/// call and reads only the matching files.
#[tokio::test]
async fn rg_with_index_makes_one_search_and_reads_only_matches() {
    let (fs, mut bash) = setup(CountingFs::new().with_index()).await;
    for script in ["rg -l needle tree", "rg needle tree", "rg -c needle tree"] {
        let (c, out) = run(&fs, &mut bash, script).await;
        assert_eq!(line_count(&out), MATCHES, "{script}");
        assert_eq!(c.search, 1, "{script}: {c}");
        assert_eq!(c.read_file, MATCHES, "{script}: {c}");
        assert!(c.read_dir <= INDEX_LISTING_BUDGET, "{script}: {c}");
        assert!(c.metadata() <= 2, "{script}: {c}");
    }
}

#[tokio::test]
async fn rg_with_index_and_no_ignore_lists_nothing() {
    let (fs, mut bash) = setup(CountingFs::new().with_index()).await;
    let (c, out) = run(&fs, &mut bash, "rg -l --no-ignore needle tree").await;
    assert_eq!(line_count(&out), MATCHES);
    assert_eq!(c.search, 1, "{c}");
    assert_eq!(c.read_file, MATCHES, "{c}");
    assert_eq!(c.read_dir, 0, "{c}");
}

/// Without an index, the walk lists each directory once and learns from the
/// listing which ignore files exist: no `stat` probes per directory.
#[tokio::test]
async fn rg_walk_lists_each_directory_once_without_probing_ignore_files() {
    let (fs, mut bash) = setup(CountingFs::new()).await;
    let (c, out) = run(&fs, &mut bash, "rg -l needle tree").await;
    assert_eq!(line_count(&out), MATCHES);
    assert_eq!(c.read_file, TOTAL_FILES, "{c}");
    // Every directory under the root, plus the two ancestors above it.
    assert_eq!(c.read_dir, DIRS + 2, "{c}");
    assert!(c.metadata() <= 2, "{c}");
}

/// Ignore rules, hidden directories and depth limits applied to index
/// matches must give exactly the walk's answer.
#[tokio::test]
async fn rg_index_results_match_walk_with_ignore_files() {
    async fn layout(fs: &dyn FileSystem) {
        seed(fs).await;
        fs.mkdir(Path::new("/workspace/tree/.git"), true)
            .await
            .unwrap();
        // Root .gitignore: whole subtree, a file glob, and a re-include.
        fs.write_file(
            Path::new("/workspace/tree/.gitignore"),
            b"d1/\nf0.txt\n!d3/s0/f0.txt\n",
        )
        .await
        .unwrap();
        // Nested .ignore and .rgignore.
        fs.write_file(Path::new("/workspace/tree/d2/.ignore"), b"s5/\n")
            .await
            .unwrap();
        fs.write_file(Path::new("/workspace/tree/d4/s0/.rgignore"), b"*.txt\n")
            .await
            .unwrap();
        // .gitignore above the search root.
        fs.write_file(Path::new("/workspace/.gitignore"), b"d6/s0/\n")
            .await
            .unwrap();
        // Hidden directory with matches.
        fs.mkdir(Path::new("/workspace/tree/.hidden"), true)
            .await
            .unwrap();
        fs.write_file(Path::new("/workspace/tree/.hidden/h.txt"), b"needle\n")
            .await
            .unwrap();
        // Extra matches at shallow depth for --max-depth.
        fs.write_file(Path::new("/workspace/tree/top.txt"), b"needle\n")
            .await
            .unwrap();
        fs.write_file(Path::new("/workspace/tree/d7/mid.txt"), b"needle\n")
            .await
            .unwrap();
    }

    let walk = Arc::new(CountingFs::new());
    layout(walk.as_ref()).await;
    let indexed = Arc::new(CountingFs::new().with_index());
    layout(indexed.as_ref()).await;
    let mut walk_bash = Bash::builder().fs(walk.clone()).cwd("/workspace").build();
    let mut index_bash = Bash::builder()
        .fs(indexed.clone())
        .cwd("/workspace")
        .build();

    for script in [
        "rg -l needle tree | sort",
        "rg -n needle tree | sort",
        "rg -l --hidden needle tree | sort",
        "rg -l --no-ignore-vcs needle tree | sort",
        "rg -l --no-ignore-dot needle tree | sort",
        "rg -l --no-ignore-parent needle tree | sort",
        "rg -l --max-depth 2 needle tree | sort",
        "rg -l -g 'f0.txt' needle tree | sort",
        "(cd tree && rg -l needle | sort)",
        "rg -l needle tree/d3 | sort",
        "rg -l needle tree/d2 tree/d7 | sort",
    ] {
        let (_, expected) = run(&walk, &mut walk_bash, script).await;
        let (c, got) = run(&indexed, &mut index_bash, script).await;
        assert_eq!(got, expected, "{script}");
        assert!(!expected.is_empty(), "{script}: vacuous");
        assert!(c.search >= 1, "{script} should use the index: {c}");
    }

    // A missing operand falls back to the walk, which reports it.
    let script = "rg -l needle tree/missing tree/d3 2>&1 | sort";
    let (_, expected) = run(&walk, &mut walk_bash, script).await;
    let (_, got) = run(&indexed, &mut index_bash, script).await;
    assert_eq!(got, expected);
    assert!(
        expected.contains("tree/missing: No such file"),
        "{expected}"
    );
}

// ---- find --------------------------------------------------------------

/// Acceptance: `find dir -type f` makes one `read_dir` per directory and no
/// per-file `lstat`.
#[tokio::test]
async fn find_lists_each_directory_once_and_never_stats_listed_entries() {
    let (fs, mut bash) = setup(CountingFs::new()).await;
    for (script, lines) in [
        ("find tree -type f", TOTAL_FILES),
        ("find tree -path '*s3*'", TOP as u64 * (FILES as u64 + 1)),
        ("find tree -name f1.txt", (TOP * SUB) as u64),
        ("find tree -ls", DIRS + TOTAL_FILES),
        ("find tree -printf '%n %p\\n'", DIRS + TOTAL_FILES),
        ("find tree -depth -type d", DIRS),
    ] {
        let (c, out) = run(&fs, &mut bash, script).await;
        assert_eq!(line_count(&out), lines, "{script}");
        assert_eq!(c.read_dir, DIRS, "{script}: {c}");
        // Only the root is looked up.
        assert!(c.metadata() <= 1, "{script}: {c}");
        assert_eq!(c.read_file, 0, "{script}: {c}");
    }
}

#[tokio::test]
async fn find_uses_one_bulk_walk_when_the_backend_has_one() {
    let (fs, mut bash) = setup(CountingFs::new().with_native_walk()).await;
    let (c, out) = run(&fs, &mut bash, "find tree -type f").await;
    assert_eq!(line_count(&out), TOTAL_FILES);
    assert_eq!(c.walk, 1, "{c}");
    assert_eq!(c.read_dir, 0, "{c}");
    assert!(c.total() <= 2, "{c}");

    // -prune cuts the traversal, so find lists lazily instead.
    let (c, out) = run(&fs, &mut bash, "find tree -name 'd*' -prune").await;
    assert_eq!(line_count(&out), TOP as u64);
    assert_eq!(c.walk, 0, "{c}");
    assert_eq!(c.read_dir, 1, "{c}");
}

/// The bulk walk and listing metadata must not change find's answers.
#[tokio::test]
async fn find_output_is_the_same_with_and_without_bulk_walk() {
    async fn layout(fs: &dyn FileSystem) {
        seed(fs).await;
        fs.symlink(Path::new("d1"), Path::new("/workspace/tree/link-d1"))
            .await
            .unwrap();
        fs.symlink(Path::new("missing"), Path::new("/workspace/tree/dangling"))
            .await
            .unwrap();
        fs.mkdir(Path::new("/workspace/tree/empty"), false)
            .await
            .unwrap();
    }
    let plain = Arc::new(CountingFs::new());
    layout(plain.as_ref()).await;
    let bulk = Arc::new(CountingFs::new().with_native_walk());
    layout(bulk.as_ref()).await;
    let mut plain_bash = Bash::builder().fs(plain.clone()).cwd("/workspace").build();
    let mut bulk_bash = Bash::builder().fs(bulk.clone()).cwd("/workspace").build();
    for script in [
        "find tree | sort",
        "find tree -type l | sort",
        "find tree -xtype d | sort",
        "find -L tree -type f | wc -l",
        "find tree -maxdepth 2 | sort",
        "find tree -mindepth 2 -maxdepth 2 -type d | sort",
        "find tree -empty",
        "find tree -printf '%y %n %s %p\\n' | sort",
        "find tree -newer tree/d0/s0/f0.txt | wc -l",
        "find tree tree/d2 -name f9.txt | sort",
    ] {
        let (_, expected) = run(&plain, &mut plain_bash, script).await;
        let (_, got) = run(&bulk, &mut bulk_bash, script).await;
        assert_eq!(got, expected, "{script}");
        assert!(!expected.is_empty(), "{script}: vacuous");
    }
}

// ---- other tree walkers -------------------------------------------------

#[tokio::test]
async fn grep_recursive_reads_files_without_a_stat_each() {
    let (fs, mut bash) = setup(CountingFs::new()).await;
    let (c, out) = run(&fs, &mut bash, "grep -rl needle tree").await;
    assert_eq!(line_count(&out), MATCHES);
    assert_eq!(c.read_dir, DIRS, "{c}");
    assert_eq!(c.read_file, TOTAL_FILES, "{c}");
    assert!(c.metadata() <= 3, "{c}");
}

#[tokio::test]
async fn grep_recursive_uses_the_index() {
    let (fs, mut bash) = setup(CountingFs::new().with_index()).await;
    let (c, out) = run(&fs, &mut bash, "grep -rn needle tree").await;
    assert_eq!(line_count(&out), MATCHES);
    assert_eq!(c.search, 1, "{c}");
    assert_eq!(c.read_file, MATCHES, "{c}");
    assert_eq!(c.read_dir, 0, "{c}");
}

#[tokio::test]
async fn du_takes_sizes_from_listings() {
    let (fs, mut bash) = setup(CountingFs::new()).await;
    for (script, lines) in [("du -s tree", 1), ("du -a tree", DIRS + TOTAL_FILES)] {
        let (c, out) = run(&fs, &mut bash, script).await;
        assert_eq!(line_count(&out), lines, "{script}");
        assert_eq!(c.read_dir, DIRS, "{script}: {c}");
        assert!(c.metadata() <= 1, "{script}: {c}");
    }

    let (fs, mut bash) = setup(CountingFs::new().with_native_walk()).await;
    let (c, _) = run(&fs, &mut bash, "du -s tree").await;
    assert_eq!(c.walk, 1, "{c}");
    assert_eq!(c.read_dir, 0, "{c}");
}

#[tokio::test]
async fn ls_recursive_and_tree_list_each_directory_once() {
    let (fs, mut bash) = setup(CountingFs::new()).await;
    for script in ["ls -R tree", "ls -lR tree", "tree tree"] {
        let (c, _) = run(&fs, &mut bash, script).await;
        assert_eq!(c.read_dir, DIRS, "{script}: {c}");
        assert!(c.metadata() <= 2, "{script}: {c}");
    }
}

#[tokio::test]
async fn tar_reads_member_metadata_from_listings() {
    let (fs, mut bash) = setup(CountingFs::new()).await;
    let (c, _) = run(&fs, &mut bash, "tar -cf /tmp/t.tar tree").await;
    assert_eq!(c.read_dir, DIRS, "{c}");
    assert_eq!(c.read_file, TOTAL_FILES, "{c}");
    assert!(c.metadata() <= 3, "{c}");
}

#[tokio::test]
async fn cp_recursive_stays_linear_in_entries() {
    let (fs, mut bash) = setup(CountingFs::new()).await;
    let (c, _) = run(&fs, &mut bash, "cp -r tree /tmp/copy").await;
    assert_eq!(c.read_dir, DIRS, "{c}");
    // One source + one target lookup per file copy; a handful per directory.
    assert!(c.metadata() <= 2 * TOTAL_FILES + 8 * DIRS, "{c}");
    let (_, out) = run(&fs, &mut bash, "find /tmp/copy -type f | wc -l").await;
    assert_eq!(out.trim(), TOTAL_FILES.to_string());
}

#[tokio::test]
async fn globs_list_each_directory_at_most_twice() {
    let (fs, mut bash) = setup(CountingFs::new()).await;
    let (c, out) = run(&fs, &mut bash, "echo tree/*/*/*.txt | wc -w").await;
    assert_eq!(out.trim(), TOTAL_FILES.to_string());
    assert_eq!(c.read_dir, DIRS, "{c}");
    assert_eq!(c.metadata(), 0, "{c}");
    // TODO: `**` lists every directory in the walk and again for the next
    // component; a per-expansion listing cache would halve it.
    let (c, out) = run(
        &fs,
        &mut bash,
        "shopt -s globstar; echo tree/**/*.txt | wc -w",
    )
    .await;
    assert_eq!(out.trim(), TOTAL_FILES.to_string());
    assert!(c.read_dir <= 2 * DIRS, "{c}");
    assert_eq!(c.metadata(), 0, "{c}");
}

// ---- mounts ------------------------------------------------------------

/// A live mount with its own index answers `rg`/`grep` through the mount
/// table, with paths reported under the mount point.
#[tokio::test]
async fn index_of_a_mounted_filesystem_is_used_through_the_mount() {
    let data = Arc::new(CountingFs::new().with_index());
    data.mkdir(Path::new("/src"), true).await.unwrap();
    data.write_file(Path::new("/src/a.rs"), b"fn needle() {}\n")
        .await
        .unwrap();
    data.write_file(Path::new("/src/b.rs"), b"fn other() {}\n")
        .await
        .unwrap();
    let mut bash = Bash::new();
    bash.mount("/mnt/data", data.clone()).unwrap();

    let (c, out) = run(&data, &mut bash, "rg -l needle /mnt/data").await;
    assert_eq!(out, "/mnt/data/src/a.rs\n");
    assert_eq!(c.search, 1, "{c}");
    let (c, out) = run(&data, &mut bash, "grep -rl needle /mnt/data/src").await;
    assert_eq!(out, "/mnt/data/src/a.rs\n");
    assert_eq!(c.search, 1, "{c}");

    // The mount's root is listed with its own metadata, as `stat` reports.
    let mut bash2 = Bash::new();
    bash2.mount("/mnt/data", data.clone()).unwrap();
    let r = bash2
        .exec("mkdir -p /mnt; [ \"$(stat -c %a /mnt/data)\" = \"$(find /mnt -mindepth 1 -maxdepth 1 -printf '%m')\" ] && echo same")
        .await
        .unwrap();
    assert_eq!(r.stdout.to_string(), "same\n", "{}", r.stderr);
}

/// A subtree with another mount below it has no single index: fall back to
/// walking so the nested mount's files are still searched.
#[tokio::test]
async fn nested_mount_below_search_root_disables_the_index() {
    let outer = Arc::new(CountingFs::new().with_index());
    outer.mkdir(Path::new("/x"), true).await.unwrap();
    outer
        .write_file(Path::new("/x/outer.txt"), b"needle\n")
        .await
        .unwrap();
    let inner = Arc::new(CountingFs::new());
    inner
        .write_file(Path::new("/inner.txt"), b"needle\n")
        .await
        .unwrap();
    let mut bash = Bash::new();
    bash.mount("/mnt/outer", outer.clone()).unwrap();
    bash.mount("/mnt/outer/x/nested", inner).unwrap();

    let (c, out) = run(&outer, &mut bash, "rg -l needle /mnt/outer | sort").await;
    assert_eq!(
        out,
        "/mnt/outer/x/nested/inner.txt\n/mnt/outer/x/outer.txt\n"
    );
    assert_eq!(c.search, 0, "{c}");
}
