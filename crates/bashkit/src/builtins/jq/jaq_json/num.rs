//! Integer / decimal numbers.
use super::Rc;
use std::string::{String, ToString};
use std::vec::Vec;
use core::cmp::Ordering;
use core::fmt;
use core::hash::{Hash, Hasher};
use num_bigint::{BigInt, BigUint, Sign};
use num_traits::cast::ToPrimitive;

/// Integer / decimal number.
///
/// The speciality of this type is that numbers are distinguished into
/// integers and 64-bit floating-point numbers.
/// This allows using integers to index arrays,
/// while using floating-point numbers to do general math.
///
/// Operations on numbers follow a few principles:
/// * The sum, difference, product, and remainder of two integers is integer.
/// * Any other operation between two numbers yields a float.
#[derive(Clone, Debug)]
pub enum Num {
    /// Machine-size integer
    Int(isize),
    /// Arbitrarily large integer
    BigInt(Rc<BigInt>),
    /// Floating-point number
    Float(f64),
    /// Decimal number
    Dec(Rc<String>),
}

impl Num {
    /// Create a big integer.
    pub fn big_int(i: BigInt) -> Self {
        Self::BigInt(i.into())
    }

    pub(crate) fn from_str(s: &str) -> Self {
        Self::from_str_radix(s, 10).unwrap_or_else(|| Self::Dec(Rc::new(s.to_string())))
    }

    /// Convert from an integral type to a machine-sized or big integer.
    pub fn from_integral<T: Copy + TryInto<isize> + Into<BigInt>>(x: T) -> Self {
        x.try_into()
            .map_or_else(|_| Num::big_int(x.into()), Num::Int)
    }

    /// Try to parse an integer from a string with given radix.
    pub fn from_str_radix(i: &str, radix: u32) -> Option<Self> {
        let int = || isize::from_str_radix(i, radix).ok().map(Num::Int);
        let big = || bigint_from_str_radix(i, radix).map(Self::big_int);
        int().or_else(big)
    }

    /// Try to parse a decimal string to a [`Self::Float`], else return NaN.
    pub fn from_dec_str(n: &str) -> Self {
        // TODO: changed to NaN!
        n.parse().map_or(Self::Float(f64::NAN), Self::Float)
    }

    pub(crate) fn is_int(&self) -> bool {
        matches!(self, Self::Int(_) | Self::BigInt(_))
    }

    /// If the value is a machine-sized integer, return it, else fail.
    pub fn as_isize(&self) -> Option<isize> {
        match self {
            Self::Int(i) => Some(*i),
            Self::BigInt(i) => i.to_isize(),
            _ => None,
        }
    }

    pub(crate) fn as_pos_usize(&self) -> Option<PosUsize> {
        match self {
            Self::Int(i) => Some(PosUsize(*i >= 0, i.unsigned_abs())),
            Self::BigInt(i) => i
                .magnitude()
                .to_usize()
                .map(|u| PosUsize(i.sign() != Sign::Minus, u)),
            _ => None,
        }
    }

    /// If the value is or can be converted to float, return it, else fail.
    pub(crate) fn as_f64(&self) -> f64 {
        match self {
            Self::Int(n) => *n as f64,
            Self::BigInt(n) => n.to_f64().unwrap(),
            Self::Float(n) => *n,
            Self::Dec(n) => n.parse().unwrap(),
        }
    }

    pub(crate) fn length(&self) -> Self {
        match self {
            Self::Int(i) => Self::Int(i.abs()),
            Self::BigInt(i) => match i.sign() {
                Sign::Plus | Sign::NoSign => Self::BigInt(i.clone()),
                Sign::Minus => Self::BigInt(BigInt::from(i.magnitude().clone()).into()),
            },
            Self::Dec(n) => Self::from_dec_str(n).length(),
            Self::Float(f) => Self::Float(f.abs()),
        }
    }
}

#[derive(Copy, Clone)]
pub(crate) struct PosUsize(pub(crate) bool, pub(crate) usize);

impl PosUsize {
    pub fn wrap(&self, len: usize) -> Option<usize> {
        self.0.then_some(self.1).or_else(|| len.checked_sub(self.1))
    }
}

#[test]
fn wrap_test() {
    let len = 4;
    let pos = |i| PosUsize(true, i);
    let neg = |i| PosUsize(false, i);
    assert_eq!(pos(0).wrap(len), Some(0));
    assert_eq!(pos(8).wrap(len), Some(8));
    assert_eq!(neg(1).wrap(len), Some(3));
    assert_eq!(neg(4).wrap(len), Some(0));
    assert_eq!(neg(8).wrap(len), None);
}

fn int_or_big<const N: usize>(
    i: Option<isize>,
    x: [isize; N],
    f: fn([BigInt; N]) -> BigInt,
) -> Num {
    i.map_or_else(|| Num::big_int(f(x.map(BigInt::from))), Num::Int)
}

// Do not use `BigInt::parse_bytes`, because it accepts arbitrary underscores between digits.
// https://github.com/rust-num/num-bigint/issues/340
fn bigint_from_str_radix(s: &str, radix: u32) -> Option<BigInt> {
    use num_bigint::Sign::{Minus, Plus};
    let f = |c, sign| s.strip_prefix(c).map(|s| (sign, s));
    let (sign, num) = f('-', Minus).or_else(|| f('+', Plus)).unwrap_or((Plus, s));
    biguint_from_str_radix(num, radix).map(|bu| BigInt::from_biguint(sign, bu))
}

fn biguint_from_str_radix(s: &str, radix: u32) -> Option<BigUint> {
    assert!((2..=36).contains(&radix));
    if s.is_empty() {
        return None;
    }

    // normalize all characters to plain digit values
    let digits = s.bytes().map(|b| {
        Some(match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'z' => b - b'a' + 10,
            b'A'..=b'Z' => b - b'A' + 10,
            _ => return None,
        })
        .filter(|d| *d < radix as u8)
    });
    BigUint::from_radix_be(&digits.collect::<Option<Vec<_>>>()?, radix)
}

impl core::ops::Add for Num {
    type Output = Num;
    fn add(self, rhs: Self) -> Self::Output {
        use num_bigint::BigInt;
        use Num::*;
        match (self, rhs) {
            (Int(x), Int(y)) => int_or_big(x.checked_add(y), [x, y], |[x, y]| x + y),
            (Int(i), BigInt(b)) | (BigInt(b), Int(i)) => Self::big_int(&BigInt::from(i) + &*b),
            (Int(i), Float(f)) | (Float(f), Int(i)) => Float(f + i as f64),
            (BigInt(x), BigInt(y)) => Self::big_int(&*x + &*y),
            (BigInt(i), Float(f)) | (Float(f), BigInt(i)) => Float(f + i.to_f64().unwrap()),
            (Float(x), Float(y)) => Float(x + y),
            (Dec(n), r) => Self::from_dec_str(&n) + r,
            (l, Dec(n)) => l + Self::from_dec_str(&n),
        }
    }
}

impl core::ops::Sub for Num {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        use num_bigint::BigInt;
        use Num::*;
        match (self, rhs) {
            (Int(x), Int(y)) => int_or_big(x.checked_sub(y), [x, y], |[x, y]| x - y),
            (Int(i), BigInt(b)) => Self::big_int(&BigInt::from(i) - &*b),
            (BigInt(b), Int(i)) => Self::big_int(&*b - &BigInt::from(i)),
            (BigInt(x), BigInt(y)) => Self::big_int(&*x - &*y),
            (Float(f), Int(i)) => Float(f - i as f64),
            (Int(i), Float(f)) => Float(i as f64 - f),
            (Float(f), BigInt(i)) => Float(f - i.to_f64().unwrap()),
            (BigInt(i), Float(f)) => Float(i.to_f64().unwrap() - f),
            (Float(x), Float(y)) => Float(x - y),
            (Dec(n), r) => Self::from_dec_str(&n) - r,
            (l, Dec(n)) => l - Self::from_dec_str(&n),
        }
    }
}

impl core::ops::Mul for Num {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self::Output {
        use num_bigint::BigInt;
        use Num::*;
        match (self, rhs) {
            (Int(x), Int(y)) => int_or_big(x.checked_mul(y), [x, y], |[x, y]| x * y),

            (Int(i), BigInt(b)) | (BigInt(b), Int(i)) => Self::big_int(&BigInt::from(i) * &*b),
            (BigInt(x), BigInt(y)) => Self::big_int(&*x * &*y),
            (BigInt(i), Float(f)) | (Float(f), BigInt(i)) => Float(f * i.to_f64().unwrap()),
            (Float(f), Int(i)) | (Int(i), Float(f)) => Float(f * i as f64),
            (Float(x), Float(y)) => Float(x * y),
            (Dec(n), r) => Self::from_dec_str(&n) * r,
            (l, Dec(n)) => l * Self::from_dec_str(&n),
        }
    }
}

impl core::ops::Div for Num {
    type Output = Self;
    fn div(self, rhs: Self) -> Self::Output {
        use Num::{BigInt, Dec, Float, Int};
        match (self, rhs) {
            (Int(l), r) => Float(l as f64) / r,
            (l, Int(r)) => l / Float(r as f64),
            (BigInt(l), r) => Float(l.to_f64().unwrap()) / r,
            (l, BigInt(r)) => l / Float(r.to_f64().unwrap()),
            (Float(x), Float(y)) => Float(x / y),
            (Dec(n), r) => Self::from_dec_str(&n) / r,
            (l, Dec(n)) => l / Self::from_dec_str(&n),
        }
    }
}

impl core::ops::Rem for Num {
    type Output = Self;
    fn rem(self, rhs: Self) -> Self::Output {
        use num_bigint::BigInt;
        use Num::*;
        match (self, rhs) {
            // `x.checked_rem(y)` is `None` only for:
            //
            // - `isize::MIN % -1`: The remainder is always 0.
            // - `x % 0`: This is guarded by `Val`.
            (Int(x), Int(y)) => Int(x.checked_rem(y).unwrap_or(0)),
            (BigInt(x), BigInt(y)) => Num::big_int(&*x % &*y),
            (Int(i), BigInt(b)) => Num::big_int(&BigInt::from(i) % &*b),
            (BigInt(b), Int(i)) => Num::big_int(&*b % &BigInt::from(i)),
            (Int(i), Float(f)) => Float(i as f64 % f),
            (Float(f), Int(i)) => Float(f % i as f64),
            (BigInt(i), Float(f)) => Float(i.to_f64().unwrap() % f),
            (Float(f), BigInt(i)) => Float(f % i.to_f64().unwrap()),
            (Float(x), Float(y)) => Float(x % y),
            (Dec(n), r) => Self::from_dec_str(&n) % r,
            (l, Dec(n)) => l % Self::from_dec_str(&n),
        }
    }
}

impl core::ops::Neg for Num {
    type Output = Self;
    fn neg(self) -> Self::Output {
        match self {
            Self::Int(x) => int_or_big(x.checked_neg(), [x], |[x]| -x),
            Self::BigInt(x) => Self::big_int(-&*x),
            Self::Float(x) => Self::Float(-x),
            Self::Dec(n) => match n.strip_prefix('-') {
                Some(pos) => Self::Dec(pos.to_string().into()),
                None => Self::Dec(std::format!("-{n}").into()),
            },
        }
    }
}

impl Hash for Num {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            // hash machine-sized integers like floating-point numbers,
            // because they are also compared for equality that way
            Self::Int(i) => Self::Float(*i as f64).hash(state),
            // hash all non-finite floats, like NaN and infinity, to the same
            // note that `Val::hash` assumes that `Num::hash` always starts
            // with `state.write_u8(n)`, where `n < 2`
            Self::Float(f) => {
                state.write_u8(0);
                if f.is_finite() {
                    f.to_ne_bytes().hash(state);
                }
            }
            Self::Dec(d) => Self::from_dec_str(d).hash(state),
            Self::BigInt(i) => {
                let f = i.to_f64().unwrap();
                if f.is_finite() {
                    Self::Float(f).hash(state)
                } else {
                    state.write_u8(1);
                    i.hash(state)
                }
            }
        }
    }
}

#[test]
fn hash_nums() {
    use core::f64::consts::PI;
    use core::hash::BuildHasher;
    let h = |n| foldhash::fast::FixedState::with_seed(42).hash_one(n);

    assert_eq!(h(Num::Int(4096)), h(Num::big_int(4096.into())));
    assert_eq!(h(Num::Float(0.0)), h(Num::Int(0)));
    assert_eq!(h(Num::Float(3.0)), h(Num::big_int(3.into())));

    assert_ne!(h(Num::Float(0.2)), h(Num::Float(0.4)));
    assert_ne!(h(Num::Float(PI)), h(Num::big_int(3.into())));
    assert_ne!(h(Num::Float(0.2)), h(Num::Int(1)));
    assert_ne!(h(Num::Int(1)), h(Num::Int(2)));
}

impl PartialEq for Num {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Int(x), Self::Int(y)) => x == y,
            (Self::Int(i), Self::Float(f)) | (Self::Float(f), Self::Int(i)) => {
                f.is_finite() && float_eq(*i as f64, *f)
            }
            (Self::BigInt(x), Self::BigInt(y)) => x == y,
            (Self::Int(i), Self::BigInt(b)) | (Self::BigInt(b), Self::Int(i)) => {
                **b == BigInt::from(*i)
            }
            (Self::BigInt(i), Self::Float(f)) | (Self::Float(f), Self::BigInt(i)) => {
                f.is_finite() && float_eq(i.to_f64().unwrap(), *f)
            }
            (Self::Float(x), Self::Float(y)) => float_eq(*x, *y),
            (Self::Dec(x), Self::Dec(y)) if Rc::ptr_eq(x, y) => true,
            (Self::Dec(n), y) => &Self::from_dec_str(n) == y,
            (x, Self::Dec(n)) => x == &Self::from_dec_str(n),
        }
    }
}

impl Eq for Num {}

impl PartialOrd for Num {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Num {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Int(x), Self::Int(y)) => x.cmp(y),
            (Self::Int(x), Self::BigInt(y)) => BigInt::from(*x).cmp(y),
            (Self::Int(i), Self::Float(f)) => float_cmp(*i as f64, *f),
            (Self::BigInt(x), Self::Int(y)) => (**x).cmp(&BigInt::from(*y)),
            (Self::BigInt(x), Self::BigInt(y)) => x.cmp(y),
            // BigInt::to_f64 always yields Some, large values become f64::INFINITY
            (Self::BigInt(x), Self::Float(y)) => float_cmp(x.to_f64().unwrap(), *y),
            (Self::Float(f), Self::Int(i)) => float_cmp(*f, *i as f64),
            (Self::Float(x), Self::BigInt(y)) => float_cmp(*x, y.to_f64().unwrap()),
            (Self::Float(x), Self::Float(y)) => float_cmp(*x, *y),
            (Self::Dec(x), Self::Dec(y)) if Rc::ptr_eq(x, y) => Ordering::Equal,
            (Self::Dec(n), y) => Self::from_dec_str(n).cmp(y),
            (x, Self::Dec(n)) => x.cmp(&Self::from_dec_str(n)),
        }
    }
}

fn float_eq(left: f64, right: f64) -> bool {
    float_cmp(left, right) == Ordering::Equal
}

fn float_cmp(left: f64, right: f64) -> Ordering {
    if left == 0. && right == 0. {
        // consider negative and positive 0 as equal
        Ordering::Equal
    } else if left.is_nan() {
        // there are more than 50 shades of NaN, and which of these
        // you strike when you perform a calculation is not deterministic (!),
        // therefore `total_cmp` may yield different results for the same calculation
        // so we bite the bullet and handle this like in jq
        Ordering::Less
    } else if right.is_nan() {
        Ordering::Greater
    } else {
        f64::total_cmp(&left, &right)
    }
}

impl fmt::Display for Num {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Int(i) => write!(f, "{i}"),
            Self::BigInt(i) => write!(f, "{i}"),
            // BASHKIT PATCH: print floats like jq 1.7 (`1024`, `1e+17`,
            // `1e-05`, `-0`; NaN as null, infinities as the largest double).
            Self::Float(x) => f.write_str(&jq_format_f64(*x)),
            Self::Dec(n) => write!(f, "{n}"),
        }
    }
}

/// BASHKIT PATCH: jq's double formatting (`jvp_dtoa_fmt`): shortest
/// round-trip digits, plain notation unless the decimal exponent is below
/// -3 or more than 15 past the last digit, then `d.ddde±XX`.
pub(crate) fn jq_format_f64(x: f64) -> String {
    if x.is_nan() {
        return "null".into();
    }
    let x = if x.is_infinite() { f64::MAX.copysign(x) } else { x };
    let sign = if x.is_sign_negative() { "-" } else { "" };
    if x == 0.0 {
        return format!("{sign}0");
    }
    let sci = format!("{:e}", x.abs());
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let nd = digits.len() as i64;
    let decpt = exp.parse::<i64>().unwrap_or(0) + 1;
    if decpt <= -4 || decpt > nd + 15 {
        let e = decpt - 1;
        let (head, tail) = digits.split_at(1);
        let frac = if tail.is_empty() { String::new() } else { format!(".{tail}") };
        let esign = if e < 0 { '-' } else { '+' };
        format!("{sign}{head}{frac}e{esign}{:02}", e.abs())
    } else if decpt <= 0 {
        format!("{sign}0.{}{digits}", "0".repeat((-decpt) as usize))
    } else if decpt >= nd {
        format!("{sign}{digits}{}", "0".repeat((decpt - nd) as usize))
    } else {
        let (a, b) = digits.split_at(decpt as usize);
        format!("{sign}{a}.{b}")
    }
}

#[test]
fn jq_format_f64_matches_jq() {
    let cases: &[(f64, &str)] = &[
        (1e17, "1e+17"),
        (1e16, "1e+16"),
        (1e15 + 1.0, "1000000000000001"),
        (1e19, "1e+19"),
        (123456789012345678.0, "123456789012345680"),
        (0.0001, "0.0001"),
        (0.00001, "1e-05"),
        (3.0, "3"),
        (1.0 / 3.0, "0.3333333333333333"),
        (1e300, "1e+300"),
        (1.5e300, "1.5e+300"),
        (0.1 + 0.2, "0.30000000000000004"),
        (1e20, "1e+20"),
        (12345678901234567890123.0, "12345678901234568000000"),
        (f64::MAX, "1.7976931348623157e+308"),
        (5e-324, "5e-324"),
        (1e15 + 0.5, "1000000000000000.5"),
        (-2.5, "-2.5"),
        (-0.0, "-0"),
        (0.0, "0"),
        (f64::INFINITY, "1.7976931348623157e+308"),
        (f64::NEG_INFINITY, "-1.7976931348623157e+308"),
        (f64::NAN, "null"),
    ];
    for (x, want) in cases {
        assert_eq!(jq_format_f64(*x), *want, "{x}");
    }
}
