//! gawk's unsorted `for (k in a)` order.
//!
//! Decisions:
//! - Debian's awk is gawk, and scripts (and their expected output) depend on
//!   the order `for (k in a)` visits keys without `PROCINFO["sorted_in"]`.
//!   That order falls out of gawk's array storage, so this module models the
//!   storage, not just a sort: the first key stored in an empty array picks
//!   the kind, as in gawk 5.2:
//!   - **cint** (first key a non-negative integer): integers are kept by
//!     value and listed ascending; other keys spill into an extra array
//!     that is listed first. Sparse integers spill too once the integer
//!     storage would waste more than 2048 slots.
//!   - **int** (first key a negative integer): a chained hash of integers,
//!     two per bucket, plus a string array for other keys, listed first.
//!   - **str**: a chained hash with sdbm hashing (32-bit), 13/127/1021/...
//!     buckets, growth when keys per bucket exceed 2, new entries at the
//!     bucket head. Listing walks buckets in ascending order.
//! - The model replays the surviving keys in insertion order. gawk's order
//!   also depends on deleted keys (tables never shrink until empty), so after
//!   deletes the order can differ (L-AWK-001). Behavior was derived from
//!   observed output and gawk's documented design; no gawk code is copied.
//! - Cost is O(n) per loop start, like the list gawk builds itself.

/// Keys of an array (insertion order) in gawk's unsorted order.
pub(super) fn gawk_order(keys: Vec<String>) -> Vec<String> {
    let Some(first) = keys.first() else {
        return keys;
    };
    match int_key(first) {
        Some(n) if n >= 0 => cint_order(keys),
        Some(_) => {
            let mut a = IntArray::default();
            for k in keys {
                a.insert(k);
            }
            a.list()
        }
        None => {
            let mut a = StrArray::default();
            for k in keys {
                a.insert(k);
            }
            a.list()
        }
    }
}

/// gawk treats a subscript as an integer when it is the canonical decimal
/// form of a 32-bit integer (`"3"`, `"-3"`; not `"03"`, `"-0"`, `"+3"`).
fn int_key(k: &str) -> Option<i64> {
    let digits = k.strip_prefix('-').unwrap_or(k);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if digits.len() > 1 && digits.starts_with('0') || k == "-0" {
        return None;
    }
    let n: i64 = k.parse().ok()?;
    (i64::from(i32::MIN)..=i64::from(i32::MAX))
        .contains(&n)
        .then_some(n)
}

const SIZES: [usize; 21] = [
    13, 127, 1021, 8191, 16381, 32749, 65497, 131101, 262147, 524309, 1048583, 2097169, 4194319,
    8388617, 16777259, 33554467, 67108879, 134217757, 268435459, 536870923, 1073741827,
];
const CHAIN_MAX: usize = 2;

fn next_size(cur: usize) -> Option<usize> {
    SIZES.iter().copied().find(|&s| s > cur)
}

fn str_hash(k: &str) -> u64 {
    let mut h: u64 = 0;
    for &b in k.as_bytes() {
        // C `char` is signed on gawk's Debian targets.
        let c = b as i8 as i64 as u64;
        let t = h << 6;
        h = c.wrapping_add(t).wrapping_add(t << 10).wrapping_sub(h) & 0xFFFF_FFFF;
    }
    h
}

/// Hash table of strings; each bucket lists its newest entry first.
#[derive(Default)]
struct StrArray {
    buckets: Vec<Vec<(u64, String)>>,
    len: usize,
}

impl StrArray {
    fn insert(&mut self, k: String) {
        if self.buckets.is_empty() {
            self.buckets = vec![Vec::new(); SIZES[0]];
        }
        let code = str_hash(&k);
        self.len += 1;
        if self.len / self.buckets.len() > CHAIN_MAX
            && let Some(n) = next_size(self.buckets.len())
        {
            let old = std::mem::replace(&mut self.buckets, vec![Vec::new(); n]);
            for (c, key) in old.into_iter().flatten() {
                self.buckets[(c % n as u64) as usize].insert(0, (c, key));
            }
        }
        let n = self.buckets.len() as u64;
        self.buckets[(code % n) as usize].insert(0, (code, k));
    }

    fn list(self) -> Vec<String> {
        self.buckets.into_iter().flatten().map(|(_, k)| k).collect()
    }
}

fn int_hash(k: i64, size: usize) -> usize {
    let mut k = k as u32;
    k ^= k << 3;
    k = k.wrapping_add(k >> 5);
    k ^= k << 4;
    k = k.wrapping_add(k >> 17);
    k ^= k << 25;
    k = k.wrapping_add(k >> 6);
    (k as usize) % size
}

/// Hash table of integers (two per bucket, head bucket filled first) plus
/// a string array for other keys.
#[derive(Default)]
struct IntArray {
    /// Bucket chains, head first; each chain link holds up to two keys.
    buckets: Vec<Vec<Vec<(i64, String)>>>,
    ints: usize,
    rest: Option<StrArray>,
}

impl IntArray {
    fn insert(&mut self, k: String) {
        let Some(n) = int_key(&k) else {
            self.rest.get_or_insert_with(StrArray::default).insert(k);
            return;
        };
        if self.buckets.is_empty() {
            self.buckets = vec![Vec::new(); SIZES[0]];
        }
        self.ints += 1;
        if self.ints / self.buckets.len() > CHAIN_MAX
            && let Some(size) = next_size(self.buckets.len())
        {
            let old = std::mem::replace(&mut self.buckets, vec![Vec::new(); size]);
            for (num, key) in old.into_iter().flatten().flatten() {
                self.put(num, key);
            }
        }
        self.put(n, k);
    }

    fn put(&mut self, n: i64, k: String) {
        let size = self.buckets.len();
        let chain = &mut self.buckets[int_hash(n, size)];
        match chain.first_mut() {
            Some(head) if head.len() < 2 => head.push((n, k)),
            _ => chain.insert(0, vec![(n, k)]),
        }
    }

    fn list(self) -> Vec<String> {
        let mut out = self.rest.map(StrArray::list).unwrap_or_default();
        out.extend(self.buckets.into_iter().flatten().flatten().map(|(_, k)| k));
        out
    }
}

const NHAT: u32 = 10;
const THRESHOLD: i64 = 1 << (NHAT + 1);

/// Storage group of a non-negative integer: everything below 2^NHAT shares
/// one, then one per power of two.
fn cint_group(k: i64) -> u32 {
    let r = 63 - (k.max(1) as u64).leading_zeros();
    if r < NHAT { NHAT } else { r + 1 }
}

/// The fixed-size slot block (base, size) that holds `k`.
fn cint_leaf(k: i64) -> (i64, i64) {
    let m = cint_group(k) - 1;
    let (mut m, mut base) = if m < NHAT { (NHAT, 0) } else { (m, 1i64 << m) };
    loop {
        let n = m.div_ceil(2);
        let size = 1i64 << n;
        base += size * ((k - base) / size);
        if n > NHAT {
            m = n;
        } else {
            return (base, size);
        }
    }
}

fn cint_order(keys: Vec<String>) -> Vec<String> {
    let mut leaves = std::collections::HashSet::new();
    let mut capacity: i64 = 0;
    let mut ints: Vec<(i64, String)> = Vec::new();
    let mut rest: Option<Rest> = None;
    for k in keys {
        if let Some(n) = int_key(&k).filter(|&n| n >= 0) {
            let m = cint_group(n) - 1;
            let mut li = m.max(NHAT);
            while li >= NHAT {
                li = li.div_ceil(2);
            }
            if capacity + (1i64 << li) - ints.len() as i64 <= THRESHOLD {
                let (base, size) = cint_leaf(n);
                if leaves.insert(base) {
                    capacity += size;
                }
                ints.push((n, k));
                continue;
            }
        }
        match &mut rest {
            Some(Rest::Int(a)) => a.insert(k),
            Some(Rest::Str(a)) => a.insert(k),
            None if int_key(&k).is_some() => {
                let mut a = IntArray::default();
                a.insert(k);
                rest = Some(Rest::Int(a));
            }
            None => {
                let mut a = StrArray::default();
                a.insert(k);
                rest = Some(Rest::Str(a));
            }
        }
    }
    ints.sort_by_key(|(n, _)| *n);
    let mut out = match rest {
        Some(Rest::Int(a)) => a.list(),
        Some(Rest::Str(a)) => a.list(),
        None => Vec::new(),
    };
    out.extend(ints.into_iter().map(|(_, k)| k));
    out
}

enum Rest {
    Int(IntArray),
    Str(StrArray),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(keys: &[&str]) -> Vec<String> {
        gawk_order(keys.iter().map(|k| k.to_string()).collect())
    }

    #[test]
    fn string_keys_follow_hash_buckets() {
        // gawk 5.2.1: marketing, support, sales, engineering.
        assert_eq!(
            order(&["engineering", "marketing", "sales", "support"]),
            ["marketing", "support", "sales", "engineering"]
        );
    }

    #[test]
    fn integer_keys_ascend() {
        assert_eq!(order(&["3", "1", "10", "2"]), ["1", "2", "3", "10"]);
    }

    #[test]
    fn non_integer_keys_list_before_integers() {
        let got = order(&["2", "x", "1"]);
        assert_eq!(got, ["x", "1", "2"]);
    }

    #[test]
    fn int_key_is_canonical_i32() {
        assert_eq!(int_key("0"), Some(0));
        assert_eq!(int_key("-3"), Some(-3));
        assert_eq!(int_key("03"), None);
        assert_eq!(int_key("-0"), None);
        assert_eq!(int_key("+3"), None);
        assert_eq!(int_key("1.5"), None);
        assert_eq!(int_key("2147483648"), None);
        assert_eq!(int_key(""), None);
    }

    #[test]
    fn every_key_listed_once() {
        let keys: Vec<String> = (0..5000).map(|i| format!("k{i}")).collect();
        let mut got = gawk_order(keys.clone());
        assert_eq!(got.len(), keys.len());
        got.sort();
        let mut want = keys;
        want.sort();
        assert_eq!(got, want);
        let ints: Vec<String> = (-3000..3000).map(|i| i.to_string()).collect();
        assert_eq!(gawk_order(ints.clone()).len(), ints.len());
        let sparse: Vec<String> = (0..200).map(|i| (i * 100_000).to_string()).collect();
        assert_eq!(gawk_order(sparse).len(), 200);
    }
}
