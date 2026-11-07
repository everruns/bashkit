//! Differential fuzz target: Monty python3 (bashkit-monty fork) vs CPython
//!
//! Builds a small Python program from the fuzz bytes, runs it through both
//! embedded runtimes, and fails when their stdout differs. Catches semantic
//! bugs in the fork (wrong results, wrong exception types) that crash-only
//! fuzzing cannot see, and regressions from local fork edits or re-syncs.
//!
//! Decision: programs come from a grammar, not raw bytes. Raw text is almost
//! always a SyntaxError, and syntax error messages legitimately differ
//! between the two parsers. Every generated statement is wrapped in
//! `try/except Exception` that prints the exception type, so one error does
//! not end the run and exception types are compared too.
//!
//! Decision: the grammar covers the subset Monty documents as supported
//! (ints, floats, str, list, tuple, dict, bool, None, operators, builtins,
//! str/list methods, slicing, f-strings, comprehensions, conditionals). Known
//! intentional differences are left out of the grammar rather than filtered
//! from output, see `KNOWN GAPS` below, so a mismatch is always a finding.
//!
//! KNOWN GAPS (not generated):
//! - `obj.method(arg)` evaluation order: CPython looks up `obj.method`
//!   before evaluating `arg` (`0 .append(len(0))` raises AttributeError);
//!   Monty evaluates the arguments first (TypeError). Found by this target;
//!   method-call arguments are bound to a temporary first
//! - sets and set ordering, `hash()`, `id()`: implementation-defined
//! - big repetition (`'a' * 10**20`): the two runtimes have different memory
//!   limits; MemoryError runs are skipped below (OverflowError is language
//!   semantics, e.g. float `**` overflow, and is compared)
//!
//! Comparison is skipped when either side does not exit 0 (time or memory
//! limit, which the two runtimes enforce differently) or its output hit the
//! stdout cap.
//!
//! Run with:
//!   cargo +nightly fuzz run python_diff_fuzz --features python,cpython -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;
use std::fmt::Write as _;
use std::time::Duration;

#[global_allocator]
static ALLOC: bashkit::MeteredAllocator = bashkit::MeteredAllocator;

const MAX_STDOUT: usize = 8192;
const MAX_DEPTH: u32 = 3;
const MAX_STATEMENTS: usize = 8;
const VARS: [&str; 3] = ["a", "b", "c"];
const INTS: [&str; 12] = [
    "0", "1", "2", "3", "7", "-1", "-5", "10", "255", "2**31", "10**20", "-(2**64)",
];
const FLOATS: [&str; 6] = ["0.5", "1.0", "-2.25", "3.14159", "1e10", "0.1"];
const STRS: [&str; 10] = [
    "''",
    "'a'",
    "'ab'",
    "'Hello'",
    "' x y '",
    "'a,b,,c'",
    "'\\u00e9t\\u00e9'",
    "'42'",
    "'-7'",
    "'A1b2'",
];
const STR_METHODS: [&str; 16] = [
    ".upper()",
    ".lower()",
    ".strip()",
    ".split(',')",
    ".split()",
    ".replace('a', 'XY')",
    ".find('b')",
    ".startswith('a')",
    ".endswith('b')",
    ".count('a')",
    ".isdigit()",
    ".title()",
    ".lstrip(' a')",
    ".partition(',')",
    ".zfill(5)",
    ".capitalize()",
];

/// Reads choices from the fuzz input; exhausted input yields zeros, so every
/// input produces a finite, valid program.
struct Gen<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Gen<'_> {
    fn byte(&mut self) -> u8 {
        let b = self.data.get(self.pos).copied().unwrap_or(0);
        self.pos += 1;
        b
    }

    fn pick<'s>(&mut self, items: &[&'s str]) -> &'s str {
        items[self.byte() as usize % items.len()]
    }

    fn small(&mut self, n: u8) -> u8 {
        self.byte() % n
    }

    fn exhausted(&self) -> bool {
        self.pos >= self.data.len()
    }

    fn atom(&mut self, out: &mut String) {
        match self.small(6) {
            0 => out.push_str(self.pick(&INTS)),
            1 => out.push_str(self.pick(&FLOATS)),
            2 => out.push_str(self.pick(&STRS)),
            3 => out.push_str(self.pick(&["True", "False", "None"])),
            _ => out.push_str(self.pick(&VARS)),
        }
    }

    fn expr(&mut self, out: &mut String, depth: u32) {
        if depth >= MAX_DEPTH || self.exhausted() {
            return self.atom(out);
        }
        let d = depth + 1;
        match self.small(18) {
            0 | 1 => self.atom(out),
            2 | 3 => {
                let op = self.pick(&[
                    "+", "-", "*", "/", "//", "%", "==", "!=", "<", "<=", ">", ">=", "and", "or",
                    "in", "not in", "&", "|", "^",
                ]);
                out.push('(');
                self.expr(out, d);
                let _ = write!(out, " {op} ");
                self.expr(out, d);
                out.push(')');
            }
            4 => {
                // Bounded power: literal exponent only.
                out.push('(');
                self.expr(out, d);
                let _ = write!(out, " ** {})", self.small(10));
            }
            5 => {
                out.push_str(self.pick(&["(-", "(not ", "(+", "(~"]));
                self.expr(out, d);
                out.push(')');
            }
            6 => {
                let f = self.pick(&[
                    "len",
                    "abs",
                    "str",
                    "int",
                    "float",
                    "bool",
                    "sorted",
                    "min",
                    "max",
                    "sum",
                    "list",
                    "repr",
                    "tuple",
                    "reversed",
                    "enumerate",
                    "type",
                ]);
                let wrap = matches!(f, "reversed" | "enumerate");
                if wrap {
                    out.push_str("list(");
                }
                let _ = write!(out, "{f}(");
                self.expr(out, d);
                out.push(')');
                if wrap {
                    out.push(')');
                }
            }
            7 => {
                self.expr(out, d);
                out.push_str(self.pick(&STR_METHODS));
            }
            8 => {
                out.push('[');
                for i in 0..self.small(4) {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    self.expr(out, d);
                }
                out.push(']');
            }
            9 => {
                out.push('(');
                self.expr(out, d);
                out.push_str(", ");
                self.expr(out, d);
                out.push(')');
            }
            10 => {
                out.push_str("{'k': ");
                self.expr(out, d);
                out.push_str(", 'j': ");
                self.expr(out, d);
                out.push('}');
            }
            11 => {
                self.expr(out, d);
                out.push('[');
                self.expr(out, d);
                out.push(']');
            }
            12 => {
                self.expr(out, d);
                let lo = self.pick(&["", "0", "1", "-1", "-2", "5"]);
                let hi = self.pick(&["", "0", "2", "-1", "10"]);
                let step = self.pick(&["", "", "1", "2", "-1", "-2"]);
                if step.is_empty() {
                    let _ = write!(out, "[{lo}:{hi}]");
                } else {
                    let _ = write!(out, "[{lo}:{hi}:{step}]");
                }
            }
            13 => {
                out.push_str(self.pick(&["f'<{", "f'{"]));
                self.expr(out, d);
                out.push_str(self.pick(&["}>'", "!r}'", ":>6}'", "}'"]));
            }
            14 => {
                out.push('(');
                self.expr(out, d);
                out.push_str(" if ");
                self.expr(out, d);
                out.push_str(" else ");
                self.expr(out, d);
                out.push(')');
            }
            15 => {
                out.push('[');
                out.push_str(self.pick(&["x", "x * 2", "str(x)", "(x, x)", "repr(x)"]));
                out.push_str(" for x in ");
                self.expr(out, d);
                if self.small(2) == 0 {
                    out.push_str(" if x");
                }
                out.push(']');
            }
            16 => {
                let _ = write!(out, "list(range({}, {}))", self.small(5), self.small(12));
            }
            _ => {
                out.push_str(self.pick(&["', '.join(", "'-'.join(", "''.join("]));
                self.expr(out, d);
                out.push(')');
            }
        }
    }

    fn statement(&mut self, out: &mut String) {
        out.push_str("try:\n    ");
        match self.small(6) {
            0 => {
                let v = self.pick(&VARS);
                let _ = write!(out, "{v} = ");
                self.expr(out, 0);
                let _ = write!(out, "\n    print(repr({v}))");
            }
            1 => {
                let v = self.pick(&VARS);
                // Argument evaluated first on purpose, see KNOWN GAPS.
                out.push_str("t = ");
                self.expr(out, 0);
                let _ = write!(out, "\n    {v}.append(t)\n    print(repr({v}))");
            }
            2 => {
                let v = self.pick(&VARS);
                let _ = write!(out, "for x in ");
                self.expr(out, 1);
                let _ = write!(out, ":\n        {v} = ({v}, x)\n    print(repr({v}))");
            }
            _ => {
                out.push_str("print(repr(");
                self.expr(out, 0);
                out.push_str("))");
            }
        }
        out.push_str("\nexcept Exception as e:\n    print(type(e).__name__)\n");
    }

    fn program(&mut self) -> String {
        let mut out = String::from("a = [1, 2, 3]\nb = 'abc'\nc = 0\n");
        let n = 1 + self.small(MAX_STATEMENTS as u8) as usize;
        for _ in 0..n {
            self.statement(&mut out);
        }
        out
    }
}

fn limits() -> bashkit::ExecutionLimits {
    bashkit::ExecutionLimits::new()
        .max_commands(20)
        .max_stdout_bytes(MAX_STDOUT)
        .max_stderr_bytes(4096)
        .timeout(Duration::from_secs(5))
}

async fn run(mut bash: bashkit::Bash, program: &str) -> Option<String> {
    bash.fs()
        .write_file(std::path::Path::new("/p.py"), program.as_bytes())
        .await
        .ok()?;
    let r = bash.exec("python3 /p.py").await.ok()?;
    let out = r.stdout.to_string();
    // A refused run compares nothing; fail loudly instead of passing vacuously.
    assert!(
        !r.stderr.contains("disabled by default"),
        "[python_diff_fuzz] python3 refused to run: {}",
        r.stderr
    );
    // Limits differ between runtimes; only clean, uncapped runs compare.
    if r.exit_code != 0
        || out.len() >= MAX_STDOUT
        || out.contains("MemoryError")
        || out.contains("RecursionError")
    {
        return None;
    }
    Some(out)
}

fuzz_target!(|data: &[u8]| {
    let program = Gen { data, pos: 0 }.program();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        bashkit::testing::fuzz_init();
        let monty = bashkit::Bash::builder()
            // Monty is opt-in per shell; without this every run hits the refusal path.
            .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
            .python_with_limits(
                bashkit::PythonLimits::default()
                    .max_duration(Duration::from_secs(1))
                    .max_memory(64 * 1024 * 1024),
            )
            .limits(limits())
            .build();
        let cpython = bashkit::Bash::builder()
            .cpython_with_limits(
                bashkit::CPythonLimits::default()
                    .max_duration(Duration::from_secs(2))
                    .max_memory(256 * 1024 * 1024)
                    .max_output(MAX_STDOUT),
            )
            .limits(limits())
            .build();

        let (Some(m), Some(c)) = (run(monty, &program).await, run(cpython, &program).await) else {
            return;
        };
        if m != c {
            let diff = m
                .lines()
                .zip(c.lines())
                .position(|(a, b)| a != b)
                .unwrap_or_else(|| m.lines().count().min(c.lines().count()));
            panic!(
                "[python_diff_fuzz] Monty and CPython disagree at output line {}\n\
                 --- program ---\n{program}\n--- monty ---\n{m}\n--- cpython ---\n{c}",
                diff + 1
            );
        }
    });
});
