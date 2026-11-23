use bashkit::{
    Bash, Builtin, BuiltinContext, ExecOptions, ExecResult, ExecutionExtensions,
    ExperimentalPlanBenchmarkControl as Mode,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Instant};
fn options(mode: Mode) -> ExecOptions {
    ExecOptions::new().extensions(ExecutionExtensions::new().with(mode))
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let checkout = std::path::PathBuf::from(
        std::env::var_os("CODE_MODE_BENCHMARK")
            .expect("set CODE_MODE_BENCHMARK to the upstream benchmark checkout"),
    );
    let fanout = std::fs::read_to_string(
        checkout.join("benches/fluency/runs/claude-haiku-5-5/bash/t01_fanout/script.sh"),
    )
    .unwrap();
    let fixture =
        std::fs::read_to_string(checkout.join("benches/fluency/fixtures/t01_fanout.json")).unwrap();
    let (inventory, fanout_expected) = inventory_fixture(&fixture);
    let cases = [
        (
            "arithmetic-function",
            "f() { local i total=0; for ((i=0;i<5000;i++)); do ((total+=i)); done; printf '%s\n' \"$total\"; }; f".to_owned(),
            "12497500\n".to_owned(),
        ),
        (
            "substitution-function",
            r#"f() { local i reply; for ((i=0;i<1000;i++)); do reply=$(printf '%s' "$i"); [[ $reply =~ ^([0-9]+)$ ]] || return 3; ((BASH_REMATCH[1] == i)) || return 4; done; }; f"#.to_owned(),
            String::new(),
        ),
        (
            "jq-function",
            r#"f() { local i; for ((i=0;i<64;i++)); do jq -r --argjson scale "$i" '.n * $scale' <<< '{"n":2}'; done; }; f"#.to_owned(),
            (0..64).map(|i| format!("{}\n", i * 2)).collect(),
        ),
        ("fanout-100", fanout, fanout_expected),
        (
            "short-function",
            "f() { printf done; }; f".to_owned(),
            "done".to_owned(),
        ),
        (
            "short-arithmetic",
            "x=1; ((x+=2)); printf '%s' \"$x\"".to_owned(),
            "3".to_owned(),
        ),
    ];
    let only = std::env::args().nth(1);
    let rounds = std::env::var("BENCH_ROUNDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);
    let mut rows = Vec::new();
    for round in 0..rounds {
        for (name, script, expected) in &cases {
            if only.as_ref().is_some_and(|filter| name != filter) {
                continue;
            }
            for mode in if round % 2 == 0 {
                [Mode::InterpreterOnly, Mode::Auto]
            } else {
                [Mode::Auto, Mode::InterpreterOnly]
            } {
                let mode_name = match mode {
                    Mode::Auto => "Auto",
                    Mode::InterpreterOnly => "InterpreterOnly",
                };
                let mut samples = Vec::new();
                for index in 0..35 {
                    let mut bash = Bash::builder()
                        .builtin(
                            "list_warehouses",
                            Box::new(InventoryCommand {
                                list: true,
                                inventory: inventory.clone(),
                            }),
                        )
                        .builtin(
                            "get_inventory",
                            Box::new(InventoryCommand {
                                list: false,
                                inventory: inventory.clone(),
                            }),
                        )
                        .build();
                    let t = Instant::now();
                    let r = bash.exec_with_options(script, options(mode)).await.unwrap();
                    let elapsed = t.elapsed().as_secs_f64() * 1e6;
                    assert_eq!(r.exit_code, 0, "{name}: {}", r.stderr);
                    assert!(r.stderr.is_empty(), "{}", r.stderr);
                    assert_eq!(r.stdout, expected.as_str(), "{name}");
                    if index >= 5 {
                        samples.push(elapsed);
                    }
                }
                samples.sort_by(f64::total_cmp);
                let row = json!({"case":name,"round":round,"mode":mode_name,"median_us":samples[samples.len()/2],"samples_us":samples});
                eprintln!("{name} {mode_name} round {round}: {} us", row["median_us"]);
                rows.push(row);
            }
        }
    }
    println!(
        "{}",
        json!({"profile":"release opt3 lto=false codegen16; same binary; fresh Bash, setup excluded, parsing/compilation/execution included","rows":rows})
    );
}

struct InventoryCommand {
    list: bool,
    inventory: Arc<BTreeMap<String, Value>>,
}

#[bashkit::async_trait]
impl Builtin for InventoryCommand {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let value = if self.list {
            assert!(ctx.args.is_empty());
            json!(self.inventory.keys().collect::<Vec<_>>())
        } else {
            assert_eq!(ctx.args.len(), 1);
            self.inventory[&ctx.args[0]].clone()
        };
        Ok(ExecResult::ok(format!("{value}\n")))
    }
}

fn inventory_fixture(fixture: &str) -> (Arc<BTreeMap<String, Value>>, String) {
    let fixture: Value = serde_json::from_str(fixture).unwrap();
    let mut inventory = BTreeMap::new();
    let mut totals = BTreeMap::<&str, i64>::new();
    for replica in 0..20 {
        for (name, stock) in fixture["inventory"].as_object().unwrap() {
            inventory.insert(format!("{name}-{replica:02}"), stock.clone());
            for item in stock.as_array().unwrap() {
                *totals.entry(item["sku"].as_str().unwrap()).or_default() +=
                    item["qty"].as_i64().unwrap();
            }
        }
    }
    assert_eq!(inventory.len(), 100);
    let mut rows = totals
        .into_iter()
        .filter(|(_, sum)| *sum >= 50)
        .collect::<Vec<_>>();
    rows.sort_unstable_by(|(a, left), (b, right)| right.cmp(left).then_with(|| a.cmp(b)));
    let expected = rows
        .into_iter()
        .map(|(sku, sum)| format!("{sku} {sum}\n"))
        .collect();
    (Arc::new(inventory), expected)
}
