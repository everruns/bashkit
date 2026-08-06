//! Fuzzes yq's YAML/JSON parser, jq evaluator boundary, serialization, and
//! atomic in-place path. Run with:
//! `cargo +nightly fuzz run yq_fuzz -- -max_total_time=300`.

#![no_main]

use libfuzzer_sys::fuzz_target;

const FILTERS: &[&str] = &[
    ".",
    ".[]?",
    "map(.)",
    "select(.)",
    "{value: .}",
    ".fuzz = true",
    "paths",
    "[range(0; 32)]",
];

fuzz_target!(|data: &[u8]| {
    let Some((&selector, payload)) = data.split_first() else {
        return;
    };
    if payload.len() > 4096 {
        return;
    }

    let input = String::from_utf8_lossy(payload);
    let escaped_input = input.replace('\'', "'\\''");
    let filter = FILTERS[usize::from(selector >> 3) % FILTERS.len()];
    let input_format = if selector & 1 == 0 { "yaml" } else { "json" };
    let output_format = if selector & 2 == 0 { "yaml" } else { "json" };
    let in_place = selector & 4 != 0;

    let script = if in_place {
        format!(
            "printf '%s' '{escaped_input}' > /tmp/fuzz-input; \
             yq -p={input_format} -o={output_format} -i '{filter}' /tmp/fuzz-input"
        )
    } else {
        format!(
            "printf '%s' '{escaped_input}' | \
             yq -p={input_format} -o={output_format} '{filter}'"
        )
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        bashkit::testing::fuzz_init();
        let mut bash = bashkit::Bash::builder()
            .limits(
                bashkit::ExecutionLimits::new()
                    .max_commands(20)
                    .max_subst_depth(3)
                    .max_stdout_bytes(4096)
                    .max_stderr_bytes(4096)
                    .max_aggregate_input_bytes(16 * 1024)
                    .max_work_units(50_000)
                    .timeout(std::time::Duration::from_millis(200)),
            )
            .build();
        let probe = bash.exec("yq -n 'null'").await.unwrap();
        assert_eq!(probe.exit_code, 0, "yq feature missing: {}", probe.stderr);
        bashkit::testing::fuzz_exec(
            &mut bash,
            &script,
            "yq_fuzz",
            &["serde_yaml_ng::", "Mapping {", "TaggedValue {"],
        )
        .await;
    });
});
