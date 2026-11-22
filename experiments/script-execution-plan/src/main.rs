use async_trait::async_trait;
use bashkit::{Bash, Builtin, BuiltinContext, ExecResult, ExecutionLimits};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, BufRead},
    sync::Arc,
    time::{Duration, Instant},
};

const FANOUT: &str = include_str!(
    "../../code-mode-language-benchmark/benches/fluency/runs/claude-haiku-5-5/bash/t01_fanout/script.sh"
);
const FIXTURE: &str =
    include_str!("../../code-mode-language-benchmark/benches/fluency/fixtures/t01_fanout.json");

#[cfg(feature = "allocation_profile")]
mod allocation_profile {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicU64, Ordering};
    pub struct Counter;
    static CALLS: AtomicU64 = AtomicU64::new(0);
    static BYTES: AtomicU64 = AtomicU64::new(0);
    unsafe impl GlobalAlloc for Counter {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            CALLS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(l.size() as u64, Ordering::Relaxed);
            unsafe { System.alloc(l) }
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            unsafe { System.dealloc(p, l) }
        }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
            CALLS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(n as u64, Ordering::Relaxed);
            unsafe { System.realloc(p, l, n) }
        }
    }
    pub fn reset() {
        CALLS.store(0, Ordering::Relaxed);
        BYTES.store(0, Ordering::Relaxed);
    }
    pub fn read() -> (u64, u64) {
        (CALLS.load(Ordering::Relaxed), BYTES.load(Ordering::Relaxed))
    }
}
#[cfg(feature = "allocation_profile")]
#[global_allocator]
static COUNTER: allocation_profile::Counter = allocation_profile::Counter;

struct Inventory {
    list: bool,
    values: Arc<BTreeMap<String, Value>>,
}
#[async_trait]
impl Builtin for Inventory {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let value = if self.list {
            json!(self.values.keys().collect::<Vec<_>>())
        } else {
            self.values[&ctx.args[0]].clone()
        };
        Ok(ExecResult::ok(format!("{value}\n")))
    }
}
fn inventory() -> (Arc<BTreeMap<String, Value>>, String) {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    let mut values = BTreeMap::new();
    let mut sums = BTreeMap::<String, i64>::new();
    for replica in 0..20 {
        for (name, stock) in fixture["inventory"].as_object().unwrap() {
            values.insert(format!("{name}-{replica:02}"), stock.clone());
            for item in stock.as_array().unwrap() {
                *sums
                    .entry(item["sku"].as_str().unwrap().into())
                    .or_default() += item["qty"].as_i64().unwrap();
            }
        }
    }
    let mut rows = sums
        .into_iter()
        .filter(|(_, n)| *n >= 50)
        .collect::<Vec<_>>();
    rows.sort_unstable_by(|(a, l), (b, r)| r.cmp(l).then_with(|| a.cmp(b)));
    let expected = rows
        .into_iter()
        .map(|(name, n)| format!("{name} {n}\n"))
        .collect();
    (Arc::new(values), expected)
}
fn limits(req: &Value) -> ExecutionLimits {
    let mut l = ExecutionLimits::new();
    if let Some(n) = req["commands"].as_u64() {
        l.max_commands = n as usize;
    }
    if let Some(n) = req["iterations"].as_u64() {
        l.max_loop_iterations = n as usize;
    }
    if let Some(n) = req["work"].as_u64() {
        l.max_work_units = n;
    }
    if let Some(n) = req["live"].as_u64() {
        l.max_live_intermediate_bytes = n;
    }
    if let Some(n) = req["timeout_ms"].as_u64() {
        l.timeout = Duration::from_millis(n);
    }
    l
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("run") {
        for line in io::stdin().lock().lines() {
            let req: Value = serde_json::from_str(&line.unwrap()).unwrap();
            let mut bash = Bash::builder().limits(limits(&req)).build();
            if req["cancelled"].as_bool() == Some(true) {
                bash.cancellation_token()
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
            let scripts = req["scripts"]
                .as_array()
                .map(|v| v.iter().map(|v| v.as_str().unwrap()).collect::<Vec<_>>())
                .unwrap_or_else(|| vec![req["script"].as_str().unwrap()]);
            let mut outputs = Vec::new();
            for script in scripts {
                let result = match bash.exec(script).await {
                    Ok(r) => {
                        json!({"stdout":r.stdout.to_string(),"stderr":r.stderr.to_string(),"code":r.exit_code})
                    }
                    Err(e) => json!({"error":e.to_string()}),
                };
                outputs.push(result);
            }
            println!("{}", json!(outputs));
        }
        return;
    }
    let straight = format!("a=0\n{}\nprintf '%s\\n' \"$a\"", "((a+=3))\n".repeat(100));
    let mut cases=vec![
        ("short_tool".to_string(),"printf 'hello\\n'".to_string(),"hello\n".to_string()),
        ("short_arithmetic".into(),"a=2; ((a+=3)); printf '%s\\n' \"$a\"".into(),"5\n".into()),
        ("straight_arithmetic".into(),straight,"300\n".into()),
        ("while_1000".into(),"a=0; while ((a<1000)); do ((a++)); done; printf '%s\\n' \"$a\"".into(),"1000\n".into()),
        ("branch_loop".into(),"total=0; for ((i=0;i<1000;i++)); do if ((i&1)); then ((total+=2)); else ((total+=i)); fi; done; printf '%s\\n' \"$total\"".into(),"250500\n".into()),
        ("assignment_loop".into(),"for ((i=0;i<1000;i++)); do a=7; done; printf '%s\\n' \"$a\"".into(),"7\n".into()),
        ("pipeline_fallback".into(),"for ((i=0;i<100;i++)); do printf 'hello\\n' | cat; done".into(),"hello\n".repeat(100)),
        ("substitution_fallback".into(),"for ((i=0;i<100;i++)); do v=$(printf hello); printf '%s\\n' \"$v\"; done".into(),"hello\n".repeat(100)),
        ("colon_fallback".into(),"for ((i=0;i<1000;i++)); do :; done".into(),"".into()),
        ("substitution_unrolled".into(),"v=$(printf hello); printf '%s\\n' \"$v\"\n".repeat(100),"hello\n".repeat(100)),
        ("mixed_script".into(),"label=total\ntotal=0\nfor ((i=0;i<1000;i++)); do ((total+=i)); done\nprintf '%s %s\\n' \"$label\" \"$total\"".into(),"total 499500\n".into()),
    ];
    for n in [100, 1000, 5000] {
        cases.push((
            format!("arithmetic_for_{n}"),
            format!(
                "total=0; for ((i=0;i<{n};i++)); do ((total+=i)); done; printf '%s\\n' \"$total\""
            ),
            format!("{}\n", n * (n - 1) / 2),
        ));
    }
    let (values, expected) = inventory();
    cases.push(("existing_fanout".into(), FANOUT.into(), expected));
    let selected = std::env::args().nth(1);
    for (name, script, expected) in cases {
        if selected
            .as_ref()
            .is_some_and(|n| n != "--same-binary" && n != &name)
        {
            continue;
        }
        #[cfg(feature = "same_binary")]
        if selected.as_deref() == Some("--same-binary") {
            use bashkit::{
                ExecOptions, ExecutionExtensions, ExperimentalPlanBenchmarkControl as Mode,
            };
            let mut ast = Vec::new();
            let mut auto = Vec::new();
            for pair in 0..210 {
                for index in 0..2 {
                    let planned = (index + pair) % 2 == 0;
                    let mut bash = if name == "existing_fanout" {
                        Bash::builder()
                            .builtin(
                                "list_warehouses",
                                Box::new(Inventory {
                                    list: true,
                                    values: values.clone(),
                                }),
                            )
                            .builtin(
                                "get_inventory",
                                Box::new(Inventory {
                                    list: false,
                                    values: values.clone(),
                                }),
                            )
                            .build()
                    } else {
                        Bash::new()
                    };
                    let mode = if planned {
                        Mode::Auto
                    } else {
                        Mode::InterpreterOnly
                    };
                    let options =
                        ExecOptions::new().extensions(ExecutionExtensions::new().with(mode));
                    let start = Instant::now();
                    let result = bash.exec_with_options(&script, options).await.unwrap();
                    let elapsed = start.elapsed().as_secs_f64() * 1e6;
                    assert_eq!(result.exit_code, 0, "{name}: {}", result.stderr);
                    assert!(result.stderr.is_empty(), "{name}: {}", result.stderr);
                    assert_eq!(result.stdout, expected.as_str(), "{name}");
                    if pair >= 10 {
                        if planned {
                            auto.push(elapsed);
                        } else {
                            ast.push(elapsed);
                        }
                    }
                }
            }
            println!("{}", json!({"workload":name,"ast_us":ast,"auto_us":auto}));
            continue;
        }
        let mut samples_us = Vec::new();
        let mut cold_samples_us = Vec::new();
        #[cfg(feature = "allocation_profile")]
        let mut allocation_calls = Vec::new();
        #[cfg(feature = "allocation_profile")]
        let mut requested_bytes = Vec::new();
        for round in 0..70 {
            let total_start = Instant::now();
            let mut bash = if name == "existing_fanout" {
                Bash::builder()
                    .builtin(
                        "list_warehouses",
                        Box::new(Inventory {
                            list: true,
                            values: values.clone(),
                        }),
                    )
                    .builtin(
                        "get_inventory",
                        Box::new(Inventory {
                            list: false,
                            values: values.clone(),
                        }),
                    )
                    .build()
            } else {
                Bash::new()
            };
            #[cfg(feature = "allocation_profile")]
            allocation_profile::reset();
            let start = Instant::now();
            let result = bash.exec(&script).await.unwrap();
            let elapsed = start.elapsed().as_secs_f64() * 1e6;
            let cold = total_start.elapsed().as_secs_f64() * 1e6;
            #[cfg(feature = "allocation_profile")]
            {
                let (calls, bytes) = allocation_profile::read();
                if round >= 10 {
                    allocation_calls.push(calls);
                    requested_bytes.push(bytes);
                }
            }
            assert_eq!(result.exit_code, 0, "{name}: {}", result.stderr);
            assert!(result.stderr.is_empty(), "{name}: {}", result.stderr);
            assert_eq!(result.stdout, expected.as_str(), "{name}");
            if round >= 10 {
                samples_us.push(elapsed);
                cold_samples_us.push(cold);
            }
        }
        let data =
            json!({"workload":name,"samples_us":samples_us,"cold_samples_us":cold_samples_us});
        #[cfg(feature = "allocation_profile")]
        let data = {
            let mut data = data;
            data["allocation_calls"] = json!(allocation_calls);
            data["requested_bytes"] = json!(requested_bytes);
            data
        };
        println!("{data}");
    }
}
