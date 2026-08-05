//! AWK runtime-regex benchmarks.
//!
//! Guards the distinction between parser-compiled top-level patterns and
//! expression/function operands compiled during evaluation. The input mirrors
//! a 50k-line access-log scan where field expressions run 300k times.
//!
//! Run with: `cargo bench --bench awk_regex`

use std::hint::black_box;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use bashkit::{Bash, FileSystem, InMemoryFs};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tokio::runtime::Runtime;

const LINES: usize = 50_000;
const FIELD_EVALUATIONS: u64 = (LINES * 6) as u64;

const EXPRESSION_ANCHORED: &str =
    r#"awk '{for(i=1;i<=NF;i++) if ($i ~ /^bytes=/) n++} END {print n}' /access.log"#;
const EXPRESSION_UNANCHORED: &str =
    r#"awk '{for(i=1;i<=NF;i++) if ($i ~ /bytes=1/) n++} END {print n}' /access.log"#;
const INDEX_PREFIX: &str =
    r#"awk '{for(i=1;i<=NF;i++) if (index($i,"bytes=")==1) n++} END {print n}' /access.log"#;
const TOP_LEVEL_PATTERN: &str = r#"awk '/bytes=1/ {n++} END {print n}' /access.log"#;

fn seed_access_log(rt: &Runtime) -> Arc<InMemoryFs> {
    let fs = Arc::new(InMemoryFs::new());
    let mut input = String::with_capacity(LINES * 48);
    for i in 0..LINES {
        use std::fmt::Write;
        writeln!(
            input,
            "2026-08-05 host{} method=GET code=200 bytes={} p/{}",
            i % 7,
            i % 1000,
            i % 50
        )
        .unwrap();
    }
    rt.block_on(async {
        let fs_dyn: Arc<dyn FileSystem> = fs.clone();
        fs_dyn
            .write_file(Path::new("/access.log"), input.as_bytes())
            .await
            .expect("write access log");
    });
    fs
}

fn bash_with(fs: &Arc<InMemoryFs>) -> Bash {
    let fs_dyn: Arc<dyn FileSystem> = fs.clone();
    Bash::builder().fs(fs_dyn).build()
}

fn bench_awk_regex(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let fs = seed_access_log(&rt);
    let mut group = c.benchmark_group("awk_regex");

    for (name, script, evaluations) in [
        (
            "expression_anchored",
            EXPRESSION_ANCHORED,
            FIELD_EVALUATIONS,
        ),
        (
            "expression_unanchored",
            EXPRESSION_UNANCHORED,
            FIELD_EVALUATIONS,
        ),
        ("index_prefix", INDEX_PREFIX, FIELD_EVALUATIONS),
        ("top_level_pattern", TOP_LEVEL_PATTERN, LINES as u64),
    ] {
        group.throughput(Throughput::Elements(evaluations));
        group.bench_function(name, |b| {
            b.to_async(&rt).iter(|| {
                let fs = fs.clone();
                async move {
                    let mut bash = bash_with(&fs);
                    black_box(bash.exec(script).await.expect("awk benchmark"));
                }
            });
        });
    }
    group.finish();
}

fn criterion_config() -> Criterion {
    Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(5))
}

#[test]
fn verify_workloads() {
    let rt = Runtime::new().unwrap();
    let fs = seed_access_log(&rt);
    rt.block_on(async {
        for (script, expected) in [
            (EXPRESSION_ANCHORED, "50000"),
            (EXPRESSION_UNANCHORED, "5550"),
            (INDEX_PREFIX, "50000"),
            (TOP_LEVEL_PATTERN, "5550"),
        ] {
            let mut bash = bash_with(&fs);
            let result = bash.exec(script).await.expect("execute workload");
            assert_eq!(result.stdout.trim(), expected);
        }
    });
}

criterion_group! {
    name = benches;
    config = criterion_config();
    targets = bench_awk_regex
}
criterion_main!(benches);
