//! factor builtin - print prime factors
//!
//! Decision: deterministic Miller-Rabin plus Pollard's rho over `u64`, so the
//! worst case (a product of two ~2^32 primes) stays in microseconds rather
//! than trial division's ~2^32 steps. Numbers above `u64::MAX` are rejected
//! (GNU accepts arbitrary precision); see L-FACTOR-001.

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// `factor` builtin.
pub struct Factor;

const HELP: &str = "Usage: factor [NUMBER]...\nPrint the prime factors of each specified integer NUMBER.\nIf none are specified on the command line, read them from standard input.\n\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n";

fn mul_mod(a: u64, b: u64, m: u64) -> u64 {
    ((u128::from(a) * u128::from(b)) % u128::from(m)) as u64
}

fn pow_mod(mut base: u64, mut exp: u64, m: u64) -> u64 {
    let mut result = 1u64;
    base %= m;
    while exp > 0 {
        if exp & 1 == 1 {
            result = mul_mod(result, base, m);
        }
        base = mul_mod(base, base, m);
        exp >>= 1;
    }
    result
}

fn is_prime(n: u64) -> bool {
    if n < 2 {
        return false;
    }
    for p in [2u64, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        if n.is_multiple_of(p) {
            return n == p;
        }
    }
    let (mut d, mut r) = (n - 1, 0);
    while d % 2 == 0 {
        d /= 2;
        r += 1;
    }
    // These bases are deterministic for all n < 2^64.
    'witness: for a in [2u64, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        let mut x = pow_mod(a, d, n);
        if x == 1 || x == n - 1 {
            continue;
        }
        for _ in 1..r {
            x = mul_mod(x, x, n);
            if x == n - 1 {
                continue 'witness;
            }
        }
        return false;
    }
    true
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// A non-trivial divisor of composite odd `n`.
fn pollard_rho(n: u64) -> u64 {
    let mut c = 1u64;
    loop {
        let f = |x: u64| ((u128::from(x) * u128::from(x) + u128::from(c)) % u128::from(n)) as u64;
        let (mut x, mut y, mut d) = (2u64, 2u64, 1u64);
        while d == 1 {
            x = f(x);
            y = f(f(y));
            d = gcd(x.abs_diff(y), n);
        }
        if d != n {
            return d;
        }
        c += 1;
    }
}

fn factorize(n: u64, out: &mut Vec<u64>) {
    if n < 2 {
        return;
    }
    let mut n = n;
    for p in [2u64, 3, 5, 7, 11, 13] {
        while n.is_multiple_of(p) {
            out.push(p);
            n /= p;
        }
    }
    if n == 1 {
        return;
    }
    if is_prime(n) {
        out.push(n);
        return;
    }
    let d = pollard_rho(n);
    factorize(d, out);
    factorize(n / d, out);
}

fn factor_line(token: &str, out: &mut String, err: &mut String) -> bool {
    let trimmed = token.strip_prefix('+').unwrap_or(token);
    match trimmed.parse::<u64>() {
        Ok(n) => {
            let mut fs = Vec::new();
            factorize(n, &mut fs);
            fs.sort_unstable();
            out.push_str(&n.to_string());
            out.push(':');
            for f in fs {
                out.push(' ');
                out.push_str(&f.to_string());
            }
            out.push('\n');
            true
        }
        Err(_) if !trimmed.is_empty() && trimmed.bytes().all(|b| b.is_ascii_digit()) => {
            err.push_str(&format!(
                "factor: '{token}' is too large (bashkit factors up to 2^64-1)\n"
            ));
            false
        }
        Err(_) => {
            err.push_str(&format!(
                "factor: '{token}' is not a valid positive integer\n"
            ));
            false
        }
    }
}

#[async_trait]
impl Builtin for Factor {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(ctx.args, HELP, Some("factor (bashkit) 0.1")) {
            return Ok(r);
        }
        let mut out = String::new();
        let mut err = String::new();
        let mut ok = true;
        let operands: Vec<&str> = ctx
            .args
            .iter()
            .map(String::as_str)
            .filter(|a| *a != "--")
            .collect();
        if operands.is_empty() {
            let input = ctx
                .stdin
                .map(|s| s.text_lossy().into_owned())
                .unwrap_or_default();
            for token in input.split_whitespace() {
                ok &= factor_line(token, &mut out, &mut err);
            }
        } else {
            for token in operands {
                ok &= factor_line(token, &mut out, &mut err);
            }
        }
        Ok(ExecResult {
            stdout: out.into(),
            stderr: err.into(),
            exit_code: i32::from(!ok),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(n: u64) -> Vec<u64> {
        let mut v = Vec::new();
        factorize(n, &mut v);
        v.sort_unstable();
        v
    }

    #[test]
    fn factors_small_and_hard_numbers() {
        assert_eq!(f(0), Vec::<u64>::new());
        assert_eq!(f(1), Vec::<u64>::new());
        assert_eq!(f(12), vec![2, 2, 3]);
        assert_eq!(f(97), vec![97]);
        // Product of two primes near 2^32: trial division would take ~4e9 steps.
        assert_eq!(
            f(4_294_967_291 * 4_294_967_279),
            vec![4_294_967_279, 4_294_967_291]
        );
        assert_eq!(f(u64::MAX), vec![3, 5, 17, 257, 641, 65537, 6_700_417]);
        assert_eq!(
            f(18_446_744_073_709_551_557),
            vec![18_446_744_073_709_551_557]
        );
    }

    #[test]
    fn rejects_bad_tokens() {
        let (mut o, mut e) = (String::new(), String::new());
        assert!(!factor_line("-3", &mut o, &mut e));
        assert!(!factor_line("99999999999999999999999", &mut o, &mut e));
        assert!(e.contains("not a valid positive integer"));
        assert!(e.contains("too large"));
    }
}
