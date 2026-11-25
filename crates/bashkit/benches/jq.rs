//! Measure Bashkit's `jq` builtin through `Bash::exec` on a fresh execution.
//!
//! Repeated, interleaved, and distinct filters exercise cache hits and churn.
//! The single-call and larger JSON cases cover workloads without reuse.
//! Run with `cargo bench -p bashkit --features jq --bench jq`.

use std::fmt::Write as _;
use std::hint::black_box;

use bashkit::Bash;
use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use tokio::runtime::Builder;

const CALLS: usize = 64;
const ROWS: usize = 4096;
const INPUT: &str = r#"{"name":"widget","qty":4}"#;
const LOOKUP: &str = r#"jq -r '.name' <<< '{"name":"widget","qty":4}'"#;

fn lookup_script(filters: impl IntoIterator<Item = (String, &'static str)>) -> (String, String) {
    let mut script = String::new();
    let mut expected = String::new();
    for (filter, output) in filters {
        writeln!(script, "jq -r '{filter}' <<< '{INPUT}'").unwrap();
        writeln!(expected, "{output}").unwrap();
    }
    (script, expected)
}

fn large_input_script() -> String {
    let rows = vec![r#"{"name":"widget","qty":4}"#; ROWS].join(",");
    format!("jq 'map(.qty) | add' <<< '[{rows}]'")
}

fn bench_jq(c: &mut Criterion) {
    let rt = Builder::new_current_thread().enable_all().build().unwrap();
    let mut group = c.benchmark_group("jq");
    let interleaved = [
        (".name", "widget"),
        (".qty", "4"),
        (".name | length", "6"),
        (".qty > 0", "true"),
    ];
    let cases = [
        (
            "single_lookup",
            (LOOKUP.to_string(), "widget\n".to_string()),
            1,
        ),
        (
            "repeated_filter",
            lookup_script((0..CALLS).map(|_| (".name # 00".to_string(), "widget"))),
            CALLS,
        ),
        (
            "interleaved_filters",
            lookup_script(
                interleaved
                    .into_iter()
                    .cycle()
                    .take(CALLS)
                    .map(|(filter, output)| (filter.to_string(), output)),
            ),
            CALLS,
        ),
        (
            "hot_and_unique_filters",
            lookup_script((0..CALLS).map(|index| {
                if index % 2 == 0 {
                    (".name".to_string(), "widget")
                } else {
                    (format!(".qty # {index:02}"), "4")
                }
            })),
            CALLS,
        ),
        (
            "distinct_filters",
            lookup_script((0..CALLS).map(|index| (format!(".name # {index:02}"), "widget"))),
            CALLS,
        ),
        (
            "aggregate_4096_rows",
            (large_input_script(), format!("{}\n", 4 * ROWS)),
            1,
        ),
        (
            "array_updates_4096",
            (
                "jq -n 'reduce range(4096) as $i ([]; .[$i] = $i) | length'".into(),
                "4096\n".into(),
            ),
            1,
        ),
        (
            "object_updates_4096",
            (
                "jq -n 'reduce range(4096) as $i ({}; .[$i|tostring] = $i) | length'".into(),
                "4096\n".into(),
            ),
            1,
        ),
        (
            "array_copy_on_write",
            (
                "jq -nc '[range(4096)] as $base | [range(64) as $i | $base | .[0] = $i | length] | add'".into(),
                "262144\n".into(),
            ),
            1,
        ),
        (
            "object_copy_on_write",
            (
                "jq -nc '(reduce range(1024) as $i ({}; .[$i|tostring] = $i)) as $base | [range(64) as $i | $base | .a = $i | length] | add'".into(),
                "65600\n".into(),
            ),
            1,
        ),
        (
            "array_concat",
            (
                "jq -n 'reduce range(1024) as $i ([]; . + [1,2,3]) | length'".into(),
                "3072\n".into(),
            ),
            1,
        ),
        (
            "object_merge",
            (
                "jq -n 'reduce range(1024) as $i ({}; .[$i|tostring] = {a:$i}) | . * {\"0\":{b:3}} | length'".into(),
                "1024\n".into(),
            ),
            1,
        ),
        (
            "sort_unique",
            (
                "jq -n '[range(4096) | . % 257] | sort | unique | length'".into(),
                "257\n".into(),
            ),
            1,
        ),
        (
            "json_round_trip",
            (
                "jq -n '[range(4096)] | tojson | fromjson | add'".into(),
                "8386560\n".into(),
            ),
            1,
        ),
        (
            "join_8192",
            (
                "jq -n '[range(8192) | tostring] | join(\",\") | length'".into(),
                "39849\n".into(),
            ),
            1,
        ),
    ];
    for (name, (script, expected), calls) in &cases {
        group.throughput(Throughput::Elements(*calls as u64));
        group.bench_function(*name, |b| {
            b.to_async(&rt).iter_batched(
                Bash::new,
                |mut bash| async move {
                    let output = bash.exec(black_box(script)).await.unwrap();
                    assert_eq!(output.exit_code, 0, "{}", output.stderr);
                    assert!(output.stderr.is_empty());
                    assert_eq!(output.stdout, expected.as_str());
                    black_box(output);
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, bench_jq);
criterion_main!(benches);
