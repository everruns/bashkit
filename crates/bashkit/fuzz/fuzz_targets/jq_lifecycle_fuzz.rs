//! Structured native jq lifecycle/resource fuzzing, independently of its parser
//! grammar. Runs with both storage backends and both shell execution modes.
//! `cargo +nightly fuzz run jq_lifecycle_fuzz --features experimental_jq`

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() > 512 {
        return;
    }
    let byte = |index| usize::from(data.get(index).copied().unwrap_or_default());
    let count = 1 + byte(1) * 8;
    let depth = [0, 1, 98, 99, 100, 101, 200, 100_000][byte(2) % 8];
    let shared_depth = 1 + byte(2) % 32;
    let filter = match byte(0) % 12 {
        0 => format!(
            "[range({count})] | .[{}] = [1] | .[1:3] = [7,8] | .[0] |= empty | length",
            count + byte(3)
        ),
        1 => format!(
            "reduce range({count}) as $i ({{}}; .[$i|tostring] = [$i]) | .[] |= (. + [3]) | length"
        ),
        2 => {
            format!("[range({count})] as $base | [$base, ($base | .[0] = 7), $base] | map(length)")
        }
        3 => format!(
            "(reduce range({count}) as $i ({{}}; .[$i|tostring] = {{a:$i}})) as $base | [$base, ($base * {{\"0\":{{b:3}}}}), $base] | map(length)"
        ),
        4 => format!(
            "reduce range({depth}) as $i (null; [.]) | {}",
            ["empty", "length", "tojson", ". + 1"][byte(3) % 4]
        ),
        5 => format!("try (reduce range({depth}) as $i (null; {{a:.}})) catch 0"),
        6 => format!("(\"[\" * {depth} + \"0\" + \"]\" * {depth}) | fromjson | length"),
        7 => format!("[range({count}) | . % 17] | sort | unique | . - [1,2,3] | length"),
        8 => {
            let depth = shared_depth;
            format!(
                "def tree: reduce range({depth}) as $i (null; [.,.]); tree as $a | tree as $b | {}",
                ["$a == $b", "[$a,$b] | sort | length", "$a | contains($b)"][byte(3) % 3]
            )
        }
        9 => format!("[range({count}) | tostring] | join(\",\") | length"),
        10 => "try ({a:[1],b:[2]} | .[] |= error(\"stop\")) catch .".into(),
        _ => {
            let depth = shared_depth;
            format!("reduce range({depth}) as $i ([]; [.,.]) | tobytes | length")
        }
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
                    .max_commands(50)
                    .max_subst_depth(3)
                    .max_stdout_bytes(4096)
                    .max_stderr_bytes(4096)
                    .max_work_units(50_000)
                    .max_live_intermediate_bytes(
                        [16_384, 65_536, 1_048_576, 32_000_000][byte(4) % 4],
                    )
                    .timeout(std::time::Duration::from_millis(200)),
            )
            .build();
        let script = format!("jq -nc '{filter}'");
        bashkit::testing::fuzz_exec(&mut bash, &script, "jq_lifecycle_fuzz", &[]).await;

        // A fixed, non-fuzzed probe verifies that error/unwind cleanup lets the
        // next host request use the same interpreter and fresh meter normally.
        let probe = "jq -n '1 + 1'";
        let recovered = bash.exec(probe).await.unwrap();
        bashkit::testing::assert_fuzz_invariants(&recovered, probe, "jq_lifecycle_recovery", &[]);
        assert_eq!(recovered.exit_code, 0, "{}", recovered.stderr);
        assert_eq!(recovered.stdout, "2\n");
    });
});
