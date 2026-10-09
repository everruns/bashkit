//! Real uutils/coreutils programs running as WebAssembly guests
//! (`wasm-coreutils` feature).
//!
//! Decisions (see `knowledge/runtimes/wasm-coreutils.md`):
//! - Precompiled programs, not reimplementations: one multicall uutils guest
//!   (`bashkit-coreutils-wasm`), compiled to Pulley at build time, runs on the
//!   shared WASI host ([`super::wasi_host`]). The wasm boundary isolates
//!   tenants in one process; no native code from the guest runs in the host.
//! - Fills gaps by default: [`MISSING_NATIVE`] lists the utilities bashkit has
//!   no native builtin for; only those (plus the `coreutils` multicall entry)
//!   are registered unless the embedder asks to replace native builtins.
//! - Each call is a fresh instance: argv[0] names the utility, exported
//!   variables plus `PWD` form the environment, stdin is the captured input.
//!   Output is captured, not streamed (same as the CPython guest).
//! - Limits default tighter than CPython's (10 s, 64 MiB): coreutils are
//!   short-lived stream filters.

use std::sync::OnceLock;
use std::time::Duration;

use async_trait::async_trait;

use super::wasi_host::{self, Call, Entry, GuestConfig, GuestRuntime, GuestState, PoolSpec};
use super::{Builtin, Context, ExecutionDeadline};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// Default wall-clock cap for one call.
const DEFAULT_MAX_DURATION: Duration = Duration::from_secs(10);
/// Default guest memory cap for one call.
const DEFAULT_MAX_MEMORY: usize = 64 * 1024 * 1024;
/// Default cap on captured stdout + stderr.
const DEFAULT_MAX_OUTPUT: usize = 16 * 1024 * 1024;
/// Guest instructions between cooperative yields (~1 ms on Pulley).
const YIELD_INTERVAL_FUEL: u64 = 100_000;
/// Request-budget work units charged per call before the guest runs.
const CALL_WORK_UNITS: u64 = 64;
/// Guest instructions per request-budget work unit.
const FUEL_PER_WORK_UNIT: u64 = 1024;

/// Instance pool: 512 concurrent calls per process; slots hold up to 256 MiB
/// (the engine's `memory_reservation`).
const POOL: PoolSpec = PoolSpec {
    slots: 512,
    max_memory: 256 << 20,
    keep_resident: 1 << 20,
    max_wasm_stack: 1 << 20,
};

/// Name of the multicall entry: `coreutils <utility> [args...]`.
pub(crate) const MULTICALL: &str = "coreutils";

/// Guest utilities with no native bashkit builtin. Registered by
/// `BashBuilder::wasm_coreutils`; a unit test keeps it in sync with both the
/// guest and the native builtin set.
pub(crate) const MISSING_NATIVE: &[&str] = &[
    "csplit",
    "dir",
    "dircolors",
    "pathchk",
    "ptx",
    "shred",
    "vdir",
];

/// Every utility the guest provides.
pub(crate) fn guest_utils() -> &'static [&'static str] {
    bashkit_coreutils_wasm::UTILS
}

/// Resource limits for wasm coreutils.
///
/// Defaults: 10 s per call, 64 MiB guest memory (at most 256 MiB honored),
/// 16 MiB of captured output. A tighter request deadline always wins.
#[derive(Debug, Clone)]
pub struct WasmCoreutilsLimits {
    /// Maximum wall-clock time per call.
    pub max_duration: Duration,
    /// Maximum guest memory in bytes.
    pub max_memory: usize,
    /// Maximum captured stdout + stderr bytes; output past this is dropped and
    /// the result is marked truncated.
    pub max_output: usize,
}

impl Default for WasmCoreutilsLimits {
    fn default() -> Self {
        Self {
            max_duration: DEFAULT_MAX_DURATION,
            max_memory: DEFAULT_MAX_MEMORY,
            max_output: DEFAULT_MAX_OUTPUT,
        }
    }
}

impl WasmCoreutilsLimits {
    /// Set max wall-clock duration per call.
    #[must_use]
    pub fn max_duration(mut self, d: Duration) -> Self {
        self.max_duration = d;
        self
    }

    /// Set max guest memory in bytes (at most 256 MiB is honored).
    #[must_use]
    pub fn max_memory(mut self, bytes: usize) -> Self {
        self.max_memory = bytes;
        self
    }

    /// Set the captured output cap in bytes.
    #[must_use]
    pub fn max_output(mut self, bytes: usize) -> Self {
        self.max_output = bytes;
        self
    }
}

static RUNTIME: OnceLock<std::result::Result<GuestRuntime, String>> = OnceLock::new();

fn runtime() -> std::result::Result<&'static GuestRuntime, &'static str> {
    RUNTIME
        .get_or_init(|| {
            GuestRuntime::new(
                bashkit_coreutils_wasm::engine_config,
                &POOL,
                bashkit_coreutils_wasm::load_module,
                |_| Ok(()),
            )
            .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(String::as_str)
}

/// One uutils program (or the `coreutils` multicall entry) run as a wasm
/// guest.
pub struct WasmCoreutil {
    /// Utility to run; `None` for the multicall entry, which takes it from the
    /// first argument.
    util: Option<&'static str>,
    limits: WasmCoreutilsLimits,
}

impl WasmCoreutil {
    /// The builtin for `util`, or `None` if the guest does not provide it.
    pub fn new(util: &str, limits: WasmCoreutilsLimits) -> Option<Self> {
        let util = guest_utils().iter().copied().find(|u| *u == util)?;
        Some(Self {
            util: Some(util),
            limits,
        })
    }

    /// The `coreutils <utility> [args...]` multicall builtin.
    pub fn multicall(limits: WasmCoreutilsLimits) -> Self {
        Self { util: None, limits }
    }

    /// Utilities the embedded guest provides, sorted.
    pub fn utilities() -> &'static [&'static str] {
        guest_utils()
    }

    /// Load the embedded module now instead of on the first call. Idempotent
    /// and process-wide; returns the error text if it cannot be loaded.
    pub fn warm_up() -> std::result::Result<(), String> {
        runtime().map(|_| ()).map_err(str::to_string)
    }
}

#[async_trait]
impl Builtin for WasmCoreutil {
    fn llm_hint(&self) -> Option<&'static str> {
        match self.util {
            None => Some(
                "coreutils: run a uutils program by name (`coreutils sort -h f`), \
                 sandboxed in WebAssembly; `coreutils --list` names them.",
            ),
            Some(_) => None,
        }
    }

    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let (util, rest) = match self.util {
            Some(u) => (u, ctx.args),
            None => match ctx.args.split_first() {
                Some((first, rest)) if first == "--list" => {
                    let _ = rest;
                    let mut out = guest_utils().join("\n");
                    out.push('\n');
                    return Ok(ExecResult::ok(out));
                }
                Some((first, rest)) => match guest_utils().iter().copied().find(|u| u == first) {
                    Some(u) => (u, rest),
                    None => {
                        return Ok(ExecResult::err(
                            format!("{MULTICALL}: {first}: unknown utility\n"),
                            127,
                        ));
                    }
                },
                None => {
                    return Ok(ExecResult::err(
                        format!("Usage: {MULTICALL} <utility> [arguments...]\n"),
                        1,
                    ));
                }
            },
        };

        let rt = match runtime() {
            Ok(rt) => rt,
            Err(e) => {
                return Ok(ExecResult::err(
                    format!("{util}: embedded coreutils unavailable: {e}\n"),
                    1,
                ));
            }
        };

        let mut timeout = self.limits.max_duration;
        if let Some(remaining) = ctx
            .execution_extension::<ExecutionDeadline>()
            .and_then(|deadline| deadline.try_with(ExecutionDeadline::remaining).ok())
        {
            timeout = timeout.min(remaining);
        }
        let budget = ctx
            .execution_budget()
            .map(|b| {
                b.try_with(Clone::clone)
                    .map_err(|_| crate::Error::Cancelled)
            })
            .transpose()?;

        let stdin = ctx.stdin.map(|s| s.as_bytes().to_vec()).unwrap_or_default();
        ctx.consume_budget_input(stdin.len())?;
        ctx.consume_budget_work(CALL_WORK_UNITS)?;

        // THREAT[TM-INF]: only exported variables reach the guest, never
        // shell-local ones.
        let mut env: Vec<(String, String)> = ctx
            .env
            .iter()
            .filter(|(k, _)| k.as_str() != "PWD" && !k.starts_with("__BASHKIT_"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        env.sort();
        env.push(("PWD".to_string(), ctx.cwd.to_string_lossy().into_owned()));

        let mut args = Vec::with_capacity(rest.len() + 1);
        args.push(util.to_string());
        args.extend(rest.iter().cloned());

        let deadline = crate::time_compat::Instant::now() + timeout;
        let state = GuestState::new(
            ctx.fs.clone(),
            GuestConfig {
                args,
                env,
                stdin,
                output_cap: self.limits.max_output,
                max_memory: rt.clamp_memory(self.limits.max_memory),
                overlay: None,
                deadline,
                #[cfg(feature = "cpython")]
                http: super::cpython::HttpState::new(
                    #[cfg(feature = "http_client")]
                    None,
                    0,
                ),
            },
        );

        let done = wasi_host::run(
            rt,
            state,
            Call {
                name: util,
                noun: "program",
                entry: Entry::Start,
                timeout,
                deadline,
                budget: budget.as_ref(),
                yield_fuel: YIELD_INTERVAL_FUEL,
            },
        )
        .await?;
        ctx.consume_budget_work(done.fuel_used / FUEL_PER_WORK_UNIT)?;

        let mut stderr = done.stderr;
        let exit_code = if done.flush_failed {
            stderr
                .extend_from_slice(format!("{util}: failed to write back open files\n").as_bytes());
            if done.exit_code == 0 {
                1
            } else {
                done.exit_code
            }
        } else {
            done.exit_code
        };
        if done.truncated {
            stderr.extend_from_slice(
                format!(
                    "{util}: output truncated at {} bytes\n",
                    self.limits.max_output
                )
                .as_bytes(),
            );
        }
        let mut result = ExecResult::with_code(done.stdout, exit_code);
        result.stderr = stderr.into();
        if done.truncated {
            result.stdout_truncated = true;
            result.stderr_truncated = true;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_limits() {
        let l = WasmCoreutilsLimits::default();
        assert_eq!(l.max_duration, Duration::from_secs(10));
        assert_eq!(l.max_memory, 64 << 20);
        assert_eq!(l.max_output, 16 << 20);
        let l = l
            .max_duration(Duration::from_secs(1))
            .max_memory(1 << 20)
            .max_output(5);
        assert_eq!(
            (l.max_duration, l.max_memory, l.max_output),
            (Duration::from_secs(1), 1 << 20, 5)
        );
    }

    #[test]
    fn runtime_loads() {
        WasmCoreutil::warm_up().expect("embedded coreutils loads");
    }

    #[test]
    fn unknown_utility_is_rejected() {
        assert!(WasmCoreutil::new("bash", WasmCoreutilsLimits::default()).is_none());
        assert!(WasmCoreutil::new("sort", WasmCoreutilsLimits::default()).is_some());
    }

    /// `MISSING_NATIVE` must be exactly the guest utilities with no native
    /// builtin: adding a native builtin (or a guest utility) updates it.
    #[test]
    fn missing_native_matches_builtins() {
        let native = crate::Bash::builder().build().builtin_names();
        let expected: Vec<&str> = guest_utils()
            .iter()
            .copied()
            .filter(|u| !native.iter().any(|n| n == u))
            .collect();
        assert_eq!(MISSING_NATIVE, expected.as_slice());
    }
}
