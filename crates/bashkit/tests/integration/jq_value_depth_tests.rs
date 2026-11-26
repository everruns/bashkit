//! THREAT[TM-DOS-027] (#3013): jq values generated inside a filter must stay
//! within `MAX_VALUE_DEPTH`, or traversing/dropping them overflows the native
//! stack and kills the embedding process (`catch_unwind` cannot contain that).
//!
//! Two controls live here:
//!
//! 1. `generated_depth_bounded` re-runs the issue's 100,000-level filters, at
//!    full scale, in a supervised child on a 2 MiB embedding stack. The
//!    in-process boundary tests in `memory_growth_security_tests::jq` stay
//!    shallow on purpose; this is the one that would SIGSEGV on regression,
//!    so it must never run in the shared test process.
//! 2. `container_mutation_sites_are_reviewed` pins every place in the jq
//!    sources that creates or mutably borrows an array/object body. Depth is
//!    admitted at those sites, so a new one (typically from a jaq-json vendor
//!    sync) must be reviewed before the inventory is updated.

#![cfg(feature = "jq")]

use std::path::{Path, PathBuf};

use super::bounded_child;

const CHILD: &str = "BASHKIT_JQ_VALUE_DEPTH_CHILD";
const DEPTH_ERROR: &str = "jq: error: value nesting depth limit (100) exceeded\n";
const TRAVERSAL_CHILD: &str = "BASHKIT_JQ_TRAVERSAL_CHILD";

#[tokio::test]
async fn reader_accepts_exact_depth_limit() {
    let mut bash = bashkit::Bash::new();
    for filter in [
        "(\"[\" * 100 + \"0\" + \"]\" * 100) | fromjson | length",
        "(\"{\\\"a\\\":\" * 100 + \"0\" + \"}\" * 100) | fromjson | length",
    ] {
        let result = bash.exec(&format!("jq -n '{filter}'")).await.unwrap();
        assert_eq!(result.exit_code, 0, "{filter}: {}", result.stderr);
        assert_eq!(result.stdout, "1\n", "{filter}");
    }
}

#[test]
fn lifecycle_traversals_bounded() {
    bounded_child::run(
        "jq-traversal",
        TRAVERSAL_CHILD,
        "jq_value_depth_tests::lifecycle_traversals_child",
    );
}

#[test]
fn lifecycle_traversals_child() {
    if !bounded_child::is_child(TRAVERSAL_CHILD) {
        return;
    }
    bounded_child::on_small_stack(|| async {
        let mut bash = bashkit::Bash::builder()
            .limits(bashkit::ExecutionLimits::new().timeout(std::time::Duration::from_millis(250)))
            .build();
        let prefix = "def tree: reduce range(60) as $i (null; [.,.]); tree as $a | tree as $b | ";
        for filter in [
            format!("{prefix}$a == $b"),
            format!("{prefix}[$a,$b] | sort | length"),
            format!("{prefix}$a | contains($b)"),
            "reduce range(60) as $i ([]; [.,.]) | tobytes | length".into(),
        ] {
            let result = bash.exec(&format!("jq -n '{filter}'")).await.unwrap();
            assert_eq!(result.exit_code, 5, "{filter}: {}", result.stderr);
            assert_eq!(result.stderr, "jq: execution timed out\n", "{filter}");
            let recovered = bash.exec("jq -n '1 + 1'").await.unwrap();
            assert_eq!(recovered.stdout, "2\n");
            assert!(recovered.stderr.is_empty());
        }
    });
}

#[test]
fn generated_depth_bounded() {
    bounded_child::run(
        "jq-depth",
        CHILD,
        "jq_value_depth_tests::generated_depth_child",
    );
}

#[test]
fn generated_depth_child() {
    if !bounded_child::is_child(CHILD) {
        return;
    }
    bounded_child::on_small_stack(|| async {
        let mut bash = bashkit::Bash::new();
        // Every shape builds the deep value through a different admission
        // path: constructors, object/array index updates, slice updates,
        // `fromjson`, and cleanup while an error unwinds through `try`.
        for filter in [
            "reduce range(100000) as $i (1; [.]) | length",
            "reduce range(100000) as $i (1; {a: .}) | empty",
            "reduce range(100000) as $i ([]; .[0] = .) | length",
            "reduce range(100000) as $i ({}; .a = .) | length",
            "reduce range(100000) as $i ([]; .[0:0] = [.]) | length",
            "reduce range(100000) as $i (1; [.]) | tostring",
            "try (reduce range(100000) as $i (1; [.])) catch 0",
            "(\"[\" * 100000 + \"0\" + \"]\" * 100000) | fromjson | length",
            "(\"{\\\"a\\\":\" * 100000 + \"0\" + \"}\" * 100000) | fromjson | length",
        ] {
            let result = bash.exec(&format!("jq -n '{filter}'")).await.unwrap();
            assert_eq!(result.exit_code, 5, "{filter}: {}", result.stderr);
            assert_eq!(result.stderr, DEPTH_ERROR, "{filter}");
            assert!(result.stdout.is_empty(), "{filter}: {}", result.stdout);
            let recovered = bash.exec("jq -n '1 + 1'").await.unwrap();
            assert_eq!(recovered.exit_code, 0, "after {filter}");
            assert_eq!(recovered.stdout, "2\n");
        }
    });
}

fn jq_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            // Vendored upstream tests construct values freely.
            if path.file_name().is_some_and(|n| n == "tests") {
                continue;
            }
            jq_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && path.file_name().is_some_and(|n| n != "tests.rs")
        {
            out.push(path);
        }
    }
}

/// Reviewed sites that create or mutably reach into an array/object body,
/// as `(file relative to builtins/jq, trimmed line)`. For each one, check:
///
/// - a new container goes through `metered`/`admit_container`;
/// - an arbitrary incoming child is checked with `meter::check_depth` before
///   attachment, or the operation provably preserves the maximum depth of
///   already-admitted operands (concat, slice, merge, removal);
/// - mutable access goes through `DerefMut` so the cached depth is cleared.
///
/// See knowledge/runtimes/jaq-json-vendor.md, "Structural depth (#3013)".
const REVIEWED: &[(&str, &str)] = &[
    // Object-wide updates collect into an already checked owned body.
    ("jaq_json/mod.rs", "return Ok(Self::Obj(Rc::new(body)));"),
    // Per-run interruption reason, not value container storage.
    (
        "mod.rs",
        "let interruption = std::rc::Rc::new(std::cell::RefCell::new(None));",
    ),
    // Experimental body admission. The owned type has no mutable dereference;
    // these callers can only invoke its checked attachment operations.
    ("jaq_json/mod.rs", "return Rc::new(Container::admit(body));"),
    ("jaq_json/mod.rs", "let o = Rc::make_mut(o);"),
    ("jaq_json/mod.rs", "Rc::make_mut(a).set(i, y)?;"),
    ("jaq_json/mod.rs", "let a = Rc::make_mut(a);"),
    (
        "jaq_json/mod.rs",
        "Rc::make_mut(a).splice(skip, take, &y)?;",
    ),
    (
        "jaq_json/mod.rs",
        "return Self::Arr(Rc::new(super::owned::array(iter)));",
    ),
    ("jaq_json/mod.rs", "Rc::make_mut(&mut l).append(&r)?;"),
    ("jaq_json/mod.rs", "Rc::make_mut(&mut l).append(&r)?;"),
    ("jaq_json/mod.rs", "Rc::make_mut(&mut l).subtract(&r)?;"),
    // Reader attachments use checked push/insert; debug builds verify the
    // completed container's contextual depth before these Rc constructions.
    ("jaq_json/read.rs", "super::Rc::new(arr)"),
    (
        "jaq_json/read.rs",
        "return Ok(Val::Obj(super::Rc::new(obj)));",
    ),
    // Private mutable access stays inside owned.rs, where take replaces a
    // child with null; insertion checks depth before restoring a child.
    ("owned.rs", "Ok(Val::Obj(Rc::new(object)))"),
    ("owned.rs", "let left = Rc::make_mut(left);"),
    // Subtraction's read-only search index, pre-admitted and charged.
    (
        "owned.rs",
        "let mut sorted = Metered::new(values.iter().collect::<Vec<_>>());",
    ),
    // Ordering/hash scratch holds immutable references, pre-admitted by entries.
    ("owned.rs", "Metered::new(object.iter().collect())"),
    // Admission itself.
    ("jaq_json/mod.rs", "admit_container(Metered::new(t))"),
    ("jaq_json/mod.rs", "Rc::new(body)"),
    ("jaq_json/meter.rs", "Metered::new(v)"),
    // map_index_inner: object entry update/insert checks child depth.
    (
        "jaq_json/mod.rs",
        "match Rc::make_mut(o).entry(index.clone()) {",
    ),
    ("jaq_json/mod.rs", "let v = core::mem::take(e.get_mut());"),
    ("jaq_json/mod.rs", "Rc::make_mut(o).resync();"),
    // map_index_inner: array grow and element update check child depth.
    ("jaq_json/mod.rs", "let a = Rc::make_mut(a);"),
    ("jaq_json/mod.rs", "let a = Rc::make_mut(a);"),
    // map_range_inner: splices elements of an admitted array.
    ("jaq_json/mod.rs", "let a = Rc::make_mut(a);"),
    // Add (array, object) and Sub: depth is the max of admitted operands.
    ("jaq_json/mod.rs", "let a = Rc::make_mut(&mut l);"),
    ("jaq_json/mod.rs", "let o = Rc::make_mut(&mut l);"),
    ("jaq_json/mod.rs", "let a = Rc::make_mut(&mut l);"),
    // obj_merge: recursive merge of admitted objects keeps the max depth.
    ("jaq_json/mod.rs", "let l = Rc::make_mut(l);"),
    (
        "jaq_json/mod.rs",
        "r.for_each(|(k, v)| match (l.get_mut(&k), v) {",
    ),
    // Not containers: arbitrary-precision number text, input slot.
    (
        "jaq_json/num.rs",
        "Self::from_str_radix(s, 10).unwrap_or_else(|| Self::Dec(Rc::new(s.to_string())))",
    ),
    (
        "mod.rs",
        "std::rc::Rc::new(std::cell::RefCell::new((Val::Null, 0)));",
    ),
    (
        "convert.rs",
        "return Val::Num(super::jaq_json::Num::Dec(super::jaq_json::Rc::new(",
    ),
    (
        "convert.rs",
        "Val::Num(super::jaq_json::Num::Dec(super::jaq_json::Rc::new(s)))",
    ),
];

/// Spellings that create a container body or reach into one, anywhere in jq.
const WATCHED: &[&str] = &[
    "Rc::new(",
    "Rc::from(",
    "Rc::make_mut(",
    "Rc::get_mut(",
    "Metered::new(",
    "Metered {",
];

/// Generic in-place mutation, watched only inside the vendored value
/// implementation: outside `jaq_json/` a `Val` body is reachable mutably only
/// through the `Rc` spellings above, and these would match unrelated
/// serde/argument intermediates.
const WATCHED_IN_JAQ_JSON: &[&str] = &["get_mut(", "iter_mut(", "values_mut(", "as_mut_slice("];

#[test]
fn container_mutation_sites_are_reviewed() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/builtins/jq");
    let mut files = Vec::new();
    jq_sources(&root, &mut files);
    // Fail closed: a scan that found nothing proves nothing.
    assert!(
        files.iter().any(|f| f.ends_with("jaq_json/mod.rs")),
        "scan did not reach the vendored jaq-json sources"
    );

    let mut found = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let source = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
        for line in source.lines() {
            let code = line.trim();
            if code.starts_with("//") {
                continue;
            }
            // Ignore a trailing comment so prose cannot trigger the scan.
            let code_only = code.split(" //").next().unwrap_or(code);
            let in_jaq_json = rel.starts_with("jaq_json/");
            if WATCHED.iter().any(|w| code_only.contains(w))
                || (in_jaq_json && WATCHED_IN_JAQ_JSON.iter().any(|w| code_only.contains(w)))
            {
                found.push((rel.clone(), code.to_owned()));
            }
        }
    }

    let mut expected: Vec<(String, String)> = REVIEWED
        .iter()
        .map(|(f, l)| ((*f).to_owned(), (*l).to_owned()))
        .collect();
    found.sort();
    expected.sort();
    let unreviewed: Vec<_> = found.iter().filter(|s| !expected.contains(s)).collect();
    let stale: Vec<_> = expected.iter().filter(|s| !found.contains(s)).collect();
    assert!(
        found == expected,
        "jq container construction/mutation sites changed. Each new site must \
         admit depth (#3013, see REVIEWED docs) before it is added to the \
         inventory.\nunreviewed: {unreviewed:#?}\nno longer present: {stale:#?}"
    );
}
