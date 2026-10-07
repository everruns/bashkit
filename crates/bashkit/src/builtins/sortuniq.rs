//! Sort and uniq builtins - sort lines and filter duplicates
//!
//! Decision: `sort` mirrors GNU coreutils `sort.c` semantics in the C locale
//! (byte comparison): fields keep their leading blanks unless `-b`, a key
//! with any ordering option does not inherit global ones, `-u`/`-s` disable
//! the whole-line last-resort comparison, and `-u` keeps the first line of
//! each equal run of a stable sort. Character positions in `-k` count UTF-8
//! characters (not bytes) so keys never split a multibyte character.

use std::cmp::Ordering;

use async_trait::async_trait;

use super::{Builtin, Context, read_text_file};
use crate::error::Result;
use crate::fs::vfs_join;
use crate::interpreter::ExecResult;

/// The sort builtin - sort lines of text.
///
/// Usage: sort [OPTION]... [FILE]...
///
/// Ordering options (global or per `-k` key): `-b -d -f -g -h -i -M -n -R -r -V`.
/// Other options: `-c -C -m -o FILE -s -t SEP -k KEYDEF -u -z`, plus the
/// GNU long forms. `-S`, `-T`, `--parallel`, `--compress-program` and
/// `--batch-size` are accepted and ignored (no external resources).
pub struct Sort;

/// Ordering options shared by the global key and per-field keys.
#[derive(Clone, Default, Debug)]
struct KeyOpts {
    skip_sblanks: bool,
    skip_eblanks: bool,
    dictionary: bool,
    nonprinting: bool,
    fold: bool,
    numeric: bool,
    general: bool,
    human: bool,
    month: bool,
    version: bool,
    random: bool,
    reverse: bool,
}

impl KeyOpts {
    /// GNU `default_key_compare`: no ordering option other than `-r`.
    fn is_default(&self) -> bool {
        !(self.skip_sblanks
            || self.skip_eblanks
            || self.dictionary
            || self.nonprinting
            || self.fold
            || self.numeric
            || self.general
            || self.human
            || self.month
            || self.version
            || self.random)
    }

    /// Apply one ordering letter. `end` selects which side `b` applies to
    /// (`None` = both, as for a global option).
    fn set(&mut self, c: char, end: Option<bool>) -> bool {
        match c {
            'b' => match end {
                None => {
                    self.skip_sblanks = true;
                    self.skip_eblanks = true;
                }
                Some(false) => self.skip_sblanks = true,
                Some(true) => self.skip_eblanks = true,
            },
            'd' => self.dictionary = true,
            'f' => self.fold = true,
            'g' => self.general = true,
            'h' => self.human = true,
            'i' => self.nonprinting = true,
            'M' => self.month = true,
            'n' => self.numeric = true,
            'R' => self.random = true,
            'r' => self.reverse = true,
            'V' => self.version = true,
            _ => return false,
        }
        true
    }
}

/// A parsed sort key definition from `-k KEYDEF`.
/// Format: `F[.C][OPTS][,F[.C][OPTS]]`. Words and chars are 0-based here.
#[derive(Clone, Debug)]
struct KeySpec {
    sword: usize,
    schar: usize,
    /// `None` = key extends to end of line.
    eword: Option<usize>,
    /// 0 = end of the end field.
    echar: usize,
    opts: KeyOpts,
}

impl KeySpec {
    /// Parse a KEYDEF string like "2", "2,3", "2.3,3.4", "2n,2", "2nr".
    fn parse(spec: &str) -> std::result::Result<Self, String> {
        let bad = |why: &str| format!("sort: invalid field specification '{spec}': {why}\n");
        let (start, end) = match spec.split_once(',') {
            Some((s, e)) => (s, Some(e)),
            None => (spec, None),
        };
        let mut opts = KeyOpts::default();

        let (f, c, rest) = split_pos(start);
        let f: usize = f
            .parse()
            .map_err(|_| bad("invalid number at field start"))?;
        if f == 0 {
            return Err(bad("field number is zero"));
        }
        let schar = match c {
            Some(c) => {
                let c: usize = c.parse().map_err(|_| bad("invalid number after '.'"))?;
                if c == 0 {
                    return Err(bad("character offset is zero"));
                }
                c - 1
            }
            None => 0,
        };
        for ch in rest.chars() {
            if !opts.set(ch, Some(false)) {
                return Err(format!("sort: stray character in field spec: '{spec}'\n"));
            }
        }

        let (eword, echar) = match end {
            None => (None, 0),
            Some(e) => {
                let (f, c, rest) = split_pos(e);
                let f: usize = f.parse().map_err(|_| bad("invalid number after ','"))?;
                if f == 0 {
                    return Err(bad("field number is zero"));
                }
                let echar = match c {
                    Some(c) => c.parse().map_err(|_| bad("invalid number after '.'"))?,
                    None => 0,
                };
                for ch in rest.chars() {
                    if !opts.set(ch, Some(true)) {
                        return Err(format!("sort: stray character in field spec: '{spec}'\n"));
                    }
                }
                (Some(f - 1), echar)
            }
        };

        Ok(KeySpec {
            sword: f - 1,
            schar,
            eword,
            echar,
            opts,
        })
    }

    /// The global options as a whole-line key.
    fn whole_line(opts: KeyOpts) -> Self {
        KeySpec {
            sword: 0,
            schar: 0,
            eword: None,
            echar: 0,
            opts,
        }
    }
}

/// Split "F[.C]OPTS" into (F, Some(C), OPTS).
fn split_pos(s: &str) -> (&str, Option<&str>, &str) {
    let f_end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (f, rest) = s.split_at(f_end);
    if let Some(after) = rest.strip_prefix('.') {
        let c_end = after
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after.len());
        (f, Some(&after[..c_end]), &after[c_end..])
    } else {
        (f, None, rest)
    }
}

fn is_blank(c: char) -> bool {
    c == ' ' || c == '\t' || c == '\n'
}

fn skip_blanks(s: &str, mut pos: usize) -> usize {
    while let Some(c) = s[pos..].chars().next() {
        if !is_blank(c) {
            break;
        }
        pos += c.len_utf8();
    }
    pos
}

fn skip_nonblanks(s: &str, mut pos: usize) -> usize {
    while let Some(c) = s[pos..].chars().next() {
        if is_blank(c) {
            break;
        }
        pos += c.len_utf8();
    }
    pos
}

/// Position just after the next `tab` (or end of line) from `pos`.
fn next_tab(s: &str, pos: usize, tab: char) -> usize {
    s[pos..].find(tab).map(|i| pos + i).unwrap_or(s.len())
}

fn advance_chars(s: &str, pos: usize, n: usize) -> usize {
    s[pos..]
        .char_indices()
        .nth(n)
        .map(|(i, _)| pos + i)
        .unwrap_or(s.len())
}

/// GNU `begfield`: byte offset where the key starts.
fn begfield(line: &str, tab: Option<char>, key: &KeySpec) -> usize {
    let lim = line.len();
    let mut ptr = 0;
    let mut sword = key.sword;
    while ptr < lim && sword > 0 {
        sword -= 1;
        match tab {
            Some(t) => {
                ptr = next_tab(line, ptr, t);
                if ptr < lim {
                    ptr += t.len_utf8();
                }
            }
            None => ptr = skip_nonblanks(line, skip_blanks(line, ptr)),
        }
    }
    if key.opts.skip_sblanks {
        ptr = skip_blanks(line, ptr);
    }
    advance_chars(line, ptr, key.schar)
}

/// GNU `limfield`: byte offset where the key ends.
fn limfield(line: &str, tab: Option<char>, key: &KeySpec) -> usize {
    let lim = line.len();
    let Some(mut eword) = key.eword else {
        return lim;
    };
    let echar = key.echar;
    if echar == 0 {
        eword += 1;
    }
    let mut ptr = 0;
    while ptr < lim && eword > 0 {
        eword -= 1;
        match tab {
            Some(t) => {
                ptr = next_tab(line, ptr, t);
                if ptr < lim && (eword > 0 || echar > 0) {
                    ptr += t.len_utf8();
                }
            }
            None => ptr = skip_nonblanks(line, skip_blanks(line, ptr)),
        }
    }
    if echar != 0 {
        if key.opts.skip_eblanks {
            ptr = skip_blanks(line, ptr);
        }
        ptr = advance_chars(line, ptr, echar);
    }
    ptr
}

fn key_text<'a>(line: &'a str, tab: Option<char>, key: &KeySpec) -> &'a [u8] {
    let beg = begfield(line, tab, key);
    let lim = limfield(line, tab, key).max(beg);
    &line.as_bytes()[beg..lim]
}

fn skip_blank_bytes(s: &[u8]) -> &[u8] {
    let n = s
        .iter()
        .take_while(|&&b| b == b' ' || b == b'\t' || b == b'\n')
        .count();
    &s[n..]
}

/// Decimal number as (negative, integer digits, fraction digits), with
/// insignificant zeros stripped so magnitudes compare exactly.
fn parse_decimal(s: &[u8]) -> (bool, &[u8], &[u8]) {
    let s = skip_blank_bytes(s);
    let (neg, s) = match s.first() {
        Some(b'-') => (true, &s[1..]),
        _ => (false, s),
    };
    let int_len = s.iter().take_while(|b| b.is_ascii_digit()).count();
    let int = &s[..int_len];
    let int = &int[int.iter().take_while(|&&b| b == b'0').count()..];
    let mut frac: &[u8] = &[];
    if s.get(int_len) == Some(&b'.') {
        let f = &s[int_len + 1..];
        let flen = f.iter().take_while(|b| b.is_ascii_digit()).count();
        let f = &f[..flen];
        let trail = f.iter().rev().take_while(|&&b| b == b'0').count();
        frac = &f[..flen - trail];
    }
    let zero = int.is_empty() && frac.is_empty();
    (neg && !zero, int, frac)
}

/// GNU `numcompare`/`strnumcmp`: exact decimal comparison, `-` sign only.
fn numcompare(a: &[u8], b: &[u8]) -> Ordering {
    let (an, ai, af) = parse_decimal(a);
    let (bn, bi, bf) = parse_decimal(b);
    if an != bn {
        return if an {
            Ordering::Less
        } else {
            Ordering::Greater
        };
    }
    let mag = ai
        .len()
        .cmp(&bi.len())
        .then_with(|| ai.cmp(bi))
        .then_with(|| af.cmp(bf));
    if an { mag.reverse() } else { mag }
}

/// GNU `find_unit_order` for `-h`.
fn unit_order(s: &[u8]) -> i32 {
    let s = skip_blank_bytes(s);
    let (neg, s) = match s.first() {
        Some(b'-') => (true, &s[1..]),
        _ => (false, s),
    };
    let mut i = 0;
    let mut nonzero = false;
    while i < s.len() && s[i].is_ascii_digit() {
        nonzero |= s[i] != b'0';
        i += 1;
    }
    if s.get(i) == Some(&b'.') {
        i += 1;
        while i < s.len() && s[i].is_ascii_digit() {
            nonzero |= s[i] != b'0';
            i += 1;
        }
    }
    if !nonzero {
        return 0;
    }
    let order = match s.get(i) {
        Some(b'K') | Some(b'k') => 1,
        Some(b'M') => 2,
        Some(b'G') => 3,
        Some(b'T') => 4,
        Some(b'P') => 5,
        Some(b'E') => 6,
        Some(b'Z') => 7,
        Some(b'Y') => 8,
        Some(b'R') => 9,
        Some(b'Q') => 10,
        _ => 0,
    };
    if neg { -order } else { order }
}

fn human_compare(a: &[u8], b: &[u8]) -> Ordering {
    unit_order(a)
        .cmp(&unit_order(b))
        .then_with(|| numcompare(a, b))
}

/// `strtod`-style prefix parse: `None` when no number could be converted.
fn strtod_prefix(s: &[u8]) -> Option<f64> {
    strtod(s).map(|(v, _)| v)
}

/// C `strtod` on a byte prefix: the value and how many bytes it consumed
/// (leading blanks, sign, decimal or hex-float digits, `inf`/`infinity`/`nan`).
pub(super) fn strtod(s: &[u8]) -> Option<(f64, usize)> {
    let mut i = s
        .iter()
        .take_while(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c))
        .count();
    let mut neg = false;
    if let Some(&c) = s.get(i)
        && (c == b'+' || c == b'-')
    {
        neg = c == b'-';
        i += 1;
    }
    let rest = &s[i..];
    let lower: Vec<u8> = rest
        .iter()
        .take(3)
        .map(|b| b.to_ascii_lowercase())
        .collect();
    let sign = if neg { -1.0 } else { 1.0 };
    if lower == b"inf" {
        let long = rest.len() >= 8 && rest[..8].eq_ignore_ascii_case(b"infinity");
        return Some((sign * f64::INFINITY, i + if long { 8 } else { 3 }));
    }
    if lower == b"nan" {
        return Some((f64::NAN, i + 3));
    }
    let hex_digit = |b: Option<&u8>| b.is_some_and(|b| b.is_ascii_hexdigit());
    if rest.len() > 2
        && rest[0] == b'0'
        && (rest[1] == b'x' || rest[1] == b'X')
        && (hex_digit(rest.get(2)) || (rest.get(2) == Some(&b'.') && hex_digit(rest.get(3))))
    {
        let mut j = 2;
        let mut val = 0.0f64;
        while let Some(d) = rest.get(j).and_then(|b| (*b as char).to_digit(16)) {
            val = val * 16.0 + d as f64;
            j += 1;
        }
        if rest.get(j) == Some(&b'.') {
            j += 1;
            let mut scale = 1.0 / 16.0;
            while let Some(d) = rest.get(j).and_then(|b| (*b as char).to_digit(16)) {
                val += d as f64 * scale;
                scale /= 16.0;
                j += 1;
            }
        }
        if matches!(rest.get(j), Some(b'p') | Some(b'P')) {
            let mut k = j + 1;
            let mut eneg = false;
            if let Some(&c) = rest.get(k)
                && (c == b'+' || c == b'-')
            {
                eneg = c == b'-';
                k += 1;
            }
            let ds = rest[k..].iter().take_while(|b| b.is_ascii_digit()).count();
            if ds > 0 {
                let e: i32 = std::str::from_utf8(&rest[k..k + ds])
                    .ok()
                    .and_then(|t| t.parse().ok())
                    .unwrap_or(i32::MAX);
                val *= 2f64.powi(if eneg { -e } else { e });
                j = k + ds;
            }
        }
        return Some((sign * val, i + j));
    }
    let mut j = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    let mut digits = j;
    if rest.get(j) == Some(&b'.') {
        let f = rest[j + 1..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        digits += f;
        j += 1 + f;
    }
    if digits == 0 {
        return None;
    }
    if matches!(rest.get(j), Some(b'e') | Some(b'E')) {
        let mut k = j + 1;
        if matches!(rest.get(k), Some(b'+') | Some(b'-')) {
            k += 1;
        }
        let ds = rest[k..].iter().take_while(|b| b.is_ascii_digit()).count();
        if ds > 0 {
            j = k + ds;
        }
    }
    let text = std::str::from_utf8(&rest[..j]).ok()?;
    let text = text.strip_suffix('.').unwrap_or(text);
    let text = if text.starts_with('.') {
        format!("0{text}")
    } else {
        text.to_string()
    };
    text.parse::<f64>().ok().map(|v| (sign * v, i + j))
}

/// GNU `general_numcompare`: conversion errors < NaN < numbers.
fn general_compare(a: &[u8], b: &[u8]) -> Ordering {
    match (strtod_prefix(a), strtod_prefix(b)) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(x), Some(y)) => match (x.is_nan(), y.is_nan()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        },
    }
}

/// Month number (1-12) when the blank-trimmed text starts with an
/// abbreviated month name, else 0.
fn month_ordinal(s: &[u8]) -> u32 {
    const MONTHS: [&[u8; 3]; 12] = [
        b"JAN", b"FEB", b"MAR", b"APR", b"MAY", b"JUN", b"JUL", b"AUG", b"SEP", b"OCT", b"NOV",
        b"DEC",
    ];
    let s = skip_blank_bytes(s);
    if s.len() < 3 {
        return 0;
    }
    let head = [
        s[0].to_ascii_uppercase(),
        s[1].to_ascii_uppercase(),
        s[2].to_ascii_uppercase(),
    ];
    MONTHS
        .iter()
        .position(|m| **m == head)
        .map(|i| i as u32 + 1)
        .unwrap_or(0)
}

/// gnulib `file_prefixlen`: length without the trailing
/// `(\.[A-Za-z~][A-Za-z0-9~]*)*` suffix.
fn file_prefixlen(s: &[u8]) -> usize {
    let n = s.len();
    let mut prefixlen = 0;
    let mut i = 0;
    loop {
        if i == n {
            return prefixlen;
        }
        i += 1;
        prefixlen = i;
        while i + 1 < n && s[i] == b'.' && (s[i + 1].is_ascii_alphabetic() || s[i + 1] == b'~') {
            i += 2;
            while i < n && (s[i].is_ascii_alphanumeric() || s[i] == b'~') {
                i += 1;
            }
        }
    }
}

fn ver_order(s: &[u8], pos: usize) -> i32 {
    match s.get(pos) {
        None => -1,
        Some(c) if c.is_ascii_digit() => 0,
        Some(c) if c.is_ascii_alphabetic() => *c as i32,
        Some(b'~') => -2,
        Some(c) => *c as i32 + 256,
    }
}

/// dpkg-style `verrevcmp` used by gnulib `filevercmp`.
fn verrevcmp(a: &[u8], b: &[u8]) -> Ordering {
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        let mut first_diff = 0i32;
        while (i < a.len() && !a[i].is_ascii_digit()) || (j < b.len() && !b[j].is_ascii_digit()) {
            let (ac, bc) = (ver_order(a, i), ver_order(b, j));
            if ac != bc {
                return ac.cmp(&bc);
            }
            i += 1;
            j += 1;
        }
        while i < a.len() && a[i] == b'0' {
            i += 1;
        }
        while j < b.len() && b[j] == b'0' {
            j += 1;
        }
        while i < a.len() && j < b.len() && a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            if first_diff == 0 {
                first_diff = a[i] as i32 - b[j] as i32;
            }
            i += 1;
            j += 1;
        }
        if i < a.len() && a[i].is_ascii_digit() {
            return Ordering::Greater;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            return Ordering::Less;
        }
        if first_diff != 0 {
            return first_diff.cmp(&0);
        }
    }
    Ordering::Equal
}

/// gnulib `filevercmp` (what `sort -V` uses).
fn filevercmp(a: &[u8], b: &[u8]) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        _ => {}
    }
    if a[0] == b'.' {
        if b[0] != b'.' {
            return Ordering::Less;
        }
        let (adot, bdot) = (a == b".", b == b".");
        if adot || bdot {
            return bdot.cmp(&adot);
        }
        let (add, bdd) = (a == b"..", b == b"..");
        if add || bdd {
            return bdd.cmp(&add);
        }
    } else if b[0] == b'.' {
        return Ordering::Greater;
    }
    let (ap, bp) = (file_prefixlen(a), file_prefixlen(b));
    let one_pass = ap == a.len() && bp == b.len();
    let r = verrevcmp(&a[..ap], &b[..bp]);
    if r != Ordering::Equal || one_pass {
        r
    } else {
        verrevcmp(a, b)
    }
}

/// Compare two strings using version sort order (`sort -V`).
#[cfg(test)]
fn version_cmp(a: &str, b: &str) -> Ordering {
    filevercmp(a.as_bytes(), b.as_bytes())
}

struct SortConfig {
    keys: Vec<KeySpec>,
    tab: Option<char>,
    reverse: bool,
    unique: bool,
    stable: bool,
    random: std::collections::hash_map::RandomState,
}

impl SortConfig {
    fn compare_key(&self, a: &[u8], b: &[u8], o: &KeyOpts) -> Ordering {
        let transform = |s: &[u8]| -> Vec<u8> {
            s.iter()
                .filter(|&&c| {
                    (!o.dictionary || c.is_ascii_alphanumeric() || c == b' ' || c == b'\t')
                        && (!o.nonprinting || (0x20..0x7f).contains(&c))
                })
                .map(|&c| if o.fold { c.to_ascii_uppercase() } else { c })
                .collect()
        };
        let (ta, tb);
        let (a, b) = if o.dictionary || o.nonprinting || o.fold {
            ta = transform(a);
            tb = transform(b);
            (&ta[..], &tb[..])
        } else {
            (a, b)
        };
        if o.numeric {
            numcompare(a, b)
        } else if o.general {
            general_compare(a, b)
        } else if o.human {
            human_compare(a, b)
        } else if o.month {
            month_ordinal(a).cmp(&month_ordinal(b))
        } else if o.random {
            use std::hash::BuildHasher;
            self.random
                .hash_one(a)
                .cmp(&self.random.hash_one(b))
                .then_with(|| a.cmp(b))
        } else if o.version {
            filevercmp(a, b)
        } else {
            a.cmp(b)
        }
    }

    fn compare(&self, a: &str, b: &str) -> Ordering {
        if !self.keys.is_empty() {
            for key in &self.keys {
                let ka = key_text(a, self.tab, key);
                let kb = key_text(b, self.tab, key);
                let d = self.compare_key(ka, kb, &key.opts);
                let d = if key.opts.reverse { d.reverse() } else { d };
                if d != Ordering::Equal {
                    return d;
                }
            }
            if self.unique || self.stable {
                return Ordering::Equal;
            }
        }
        let d = a.as_bytes().cmp(b.as_bytes());
        if self.reverse { d.reverse() } else { d }
    }
}

/// Split input into records: empty records are kept (they sort first), only
/// the final terminator is dropped, and empty input has no records.
fn split_records(text: &str, sep: char) -> impl Iterator<Item = &str> {
    let body = text.strip_suffix(sep).unwrap_or(text);
    (!text.is_empty())
        .then(|| body.split(sep))
        .into_iter()
        .flatten()
}

enum CheckMode {
    Off,
    Diagnose,
    Quiet,
}

const SORT_HELP: &str = "Usage: sort [OPTION]... [FILE]...\nWrite sorted concatenation of all FILE(s) to standard output.\n\n  -b\t\tignore leading blanks\n  -d\t\tconsider only blanks and alphanumeric characters\n  -f\t\tfold lower case to upper case characters\n  -g\t\tcompare according to general numerical value\n  -i\t\tconsider only printable characters\n  -M\t\tcompare (unknown) < 'JAN' < ... < 'DEC'\n  -h\t\tcompare human readable numbers (e.g., 2K 1G)\n  -n\t\tcompare according to string numerical value\n  -R\t\tshuffle, but group identical keys\n  -r\t\treverse the result of comparisons\n  -V\t\tnatural sort of (version) numbers within text\n  --sort=WORD\tsort according to WORD\n  -c, -C\tcheck for sorted input\n  -k KEYDEF\tsort via a key definition\n  -m\t\tmerge already sorted files\n  -o FILE\twrite output to FILE\n  -s\t\tstable sort\n  -t SEP\tuse SEP as field separator\n  -u\t\toutput only the first of an equal run\n  -z\t\tline delimiter is NUL, not newline\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n";

fn sort_err(msg: impl Into<String>) -> ExecResult {
    ExecResult::err(msg.into(), 2)
}

#[async_trait]
impl Builtin for Sort {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(ctx.args, SORT_HELP, Some("sort (bashkit) 0.1"))
        {
            return Ok(r);
        }
        let mut gkey = KeyOpts::default();
        let mut unique = false;
        let mut stable = false;
        let mut check = CheckMode::Off;
        let mut merge = false;
        let mut tab: Option<char> = None;
        let mut key_specs: Vec<KeySpec> = Vec::new();
        let mut output_file: Option<String> = None;
        let mut zero_terminated = false;
        let mut files: Vec<String> = Vec::new();

        #[allow(clippy::result_large_err)]
        let set_tab = |val: &str, tab: &mut Option<char>| -> std::result::Result<(), ExecResult> {
            let c = if val == "\\0" {
                '\0'
            } else {
                let mut it = val.chars();
                match (it.next(), it.next()) {
                    (None, _) => return Err(sort_err("sort: empty tab\n")),
                    (Some(c), None) => c,
                    _ => return Err(sort_err(format!("sort: multi-character tab '{val}'\n"))),
                }
            };
            *tab = Some(c);
            Ok(())
        };

        let args = ctx.args;
        let mut i = 0;
        let mut opts_done = false;
        while i < args.len() {
            let arg = args[i].as_str();
            i += 1;
            if opts_done || arg == "-" || !arg.starts_with('-') {
                files.push(arg.to_string());
                continue;
            }
            if arg == "--" {
                opts_done = true;
                continue;
            }
            if let Some(long) = arg.strip_prefix("--") {
                let (name, inline) = match long.split_once('=') {
                    Some((n, v)) => (n, Some(v.to_string())),
                    None => (long, None),
                };
                const LONGS: &[(&str, bool)] = &[
                    ("batch-size", true),
                    ("buffer-size", true),
                    ("check", false),
                    ("compress-program", true),
                    ("debug", false),
                    ("dictionary-order", false),
                    ("field-separator", true),
                    ("general-numeric-sort", false),
                    ("human-numeric-sort", false),
                    ("ignore-case", false),
                    ("ignore-leading-blanks", false),
                    ("ignore-nonprinting", false),
                    ("key", true),
                    ("merge", false),
                    ("month-sort", false),
                    ("numeric-sort", false),
                    ("output", true),
                    ("parallel", true),
                    ("random-sort", false),
                    ("random-source", true),
                    ("reverse", false),
                    ("sort", true),
                    ("stable", false),
                    ("temporary-directory", true),
                    ("unique", false),
                    ("version-sort", false),
                    ("zero-terminated", false),
                ];
                // Unambiguous prefixes are accepted, as getopt_long does.
                let matches: Vec<&(&str, bool)> = match LONGS.iter().find(|(n, _)| *n == name) {
                    Some(m) => vec![m],
                    None => LONGS.iter().filter(|(n, _)| n.starts_with(name)).collect(),
                };
                let &(lname, takes) = match matches.as_slice() {
                    [m] => *m,
                    [] => return Ok(super::invalid_option("sort", arg, 2)),
                    _ => {
                        return Ok(sort_err(format!("sort: option '--{name}' is ambiguous\n")));
                    }
                };
                let value = if takes {
                    match inline {
                        Some(v) => v,
                        None if i < args.len() => {
                            i += 1;
                            args[i - 1].clone()
                        }
                        None => {
                            return Ok(sort_err(format!(
                                "sort: option '--{lname}' requires an argument\n"
                            )));
                        }
                    }
                } else {
                    inline.clone().unwrap_or_default()
                };
                match lname {
                    "check" => {
                        check = match value.as_str() {
                            "" | "diagnose-first" => CheckMode::Diagnose,
                            "quiet" | "silent" => CheckMode::Quiet,
                            other => {
                                return Ok(sort_err(format!(
                                    "sort: invalid argument '{other}' for '--check'\n"
                                )));
                            }
                        }
                    }
                    "dictionary-order" => gkey.dictionary = true,
                    "field-separator" => {
                        if let Err(e) = set_tab(&value, &mut tab) {
                            return Ok(e);
                        }
                    }
                    "general-numeric-sort" => gkey.general = true,
                    "human-numeric-sort" => gkey.human = true,
                    "ignore-case" => gkey.fold = true,
                    "ignore-leading-blanks" => {
                        gkey.set('b', None);
                    }
                    "ignore-nonprinting" => gkey.nonprinting = true,
                    "key" => match KeySpec::parse(&value) {
                        Ok(k) => key_specs.push(k),
                        Err(e) => return Ok(sort_err(e)),
                    },
                    "merge" => merge = true,
                    "month-sort" => gkey.month = true,
                    "numeric-sort" => gkey.numeric = true,
                    "output" => output_file = Some(value),
                    "random-sort" => gkey.random = true,
                    "reverse" => gkey.reverse = true,
                    "sort" => {
                        // Unambiguous prefixes are accepted, like `--sort=num`.
                        const WORDS: [(&str, char); 6] = [
                            ("general-numeric", 'g'),
                            ("human-numeric", 'h'),
                            ("month", 'M'),
                            ("numeric", 'n'),
                            ("random", 'R'),
                            ("version", 'V'),
                        ];
                        let hits: Vec<char> = match WORDS.iter().find(|(w, _)| *w == value) {
                            Some((_, c)) => vec![*c],
                            None => WORDS
                                .iter()
                                .filter(|(w, _)| !value.is_empty() && w.starts_with(&*value))
                                .map(|(_, c)| *c)
                                .collect(),
                        };
                        match hits.as_slice() {
                            [c] => {
                                gkey.set(*c, None);
                            }
                            _ => {
                                return Ok(sort_err(format!(
                                    "sort: invalid argument '{value}' for '--sort'\n"
                                )));
                            }
                        }
                    }
                    "stable" => stable = true,
                    "unique" => unique = true,
                    "version-sort" => gkey.version = true,
                    "zero-terminated" => zero_terminated = true,
                    // Resource-tuning options have nothing to tune here.
                    _ => {}
                }
                continue;
            }
            // Bundle of short options.
            let body = &arg[1..];
            for (pos, c) in body.char_indices() {
                match c {
                    'k' | 'o' | 't' | 'S' | 'T' | 'y' => {
                        let attached = &body[pos + c.len_utf8()..];
                        let value = if !attached.is_empty() {
                            attached.to_string()
                        } else if c == 'y' {
                            String::new()
                        } else if i < args.len() {
                            i += 1;
                            args[i - 1].clone()
                        } else {
                            return Ok(sort_err(format!(
                                "sort: option requires an argument -- '{c}'\n"
                            )));
                        };
                        match c {
                            'k' => match KeySpec::parse(&value) {
                                Ok(k) => key_specs.push(k),
                                Err(e) => return Ok(sort_err(e)),
                            },
                            'o' => output_file = Some(value),
                            't' => {
                                if let Err(e) = set_tab(&value, &mut tab) {
                                    return Ok(e);
                                }
                            }
                            _ => {}
                        }
                        break;
                    }
                    'c' => check = CheckMode::Diagnose,
                    'C' => check = CheckMode::Quiet,
                    'm' => merge = true,
                    's' => stable = true,
                    'u' => unique = true,
                    'z' => zero_terminated = true,
                    c => {
                        if !gkey.set(c, None) {
                            return Ok(super::invalid_option("sort", &format!("-{c}"), 2));
                        }
                    }
                }
            }
        }

        // Keys without ordering options inherit the global ones (GNU rule).
        for key in &mut key_specs {
            if key.opts.is_default() && !key.opts.reverse {
                key.opts = gkey.clone();
            }
        }
        if key_specs.is_empty() && !gkey.is_default() {
            key_specs.push(KeySpec::whole_line(gkey.clone()));
        }
        let cfg = SortConfig {
            keys: key_specs,
            tab,
            reverse: gkey.reverse,
            unique,
            stable,
            random: std::collections::hash_map::RandomState::new(),
        };

        if files.is_empty() {
            files.push("-".to_string());
        }
        let line_sep = if zero_terminated { '\0' } else { '\n' };
        let mut streams: Vec<Vec<String>> = Vec::new();
        for file in &files {
            let text = if file == "-" {
                ctx.stdin.map(|s| s.to_string()).unwrap_or_default()
            } else {
                let path = if file.starts_with('/') {
                    std::path::PathBuf::from(file)
                } else {
                    vfs_join(ctx.cwd, file)
                };
                match read_text_file(&*ctx.fs, &path, "sort").await {
                    Ok(t) => t,
                    Err(mut e) => {
                        e.exit_code = 2;
                        return Ok(e);
                    }
                }
            };
            streams.push(split_records(&text, line_sep).map(str::to_string).collect());
        }

        match check {
            CheckMode::Off => {}
            mode => {
                let lines = streams.concat();
                for i in 1..lines.len() {
                    let d = cfg.compare(&lines[i - 1], &lines[i]);
                    if d == Ordering::Greater || (unique && d == Ordering::Equal) {
                        let stderr = match mode {
                            CheckMode::Diagnose => {
                                format!("sort: {}:{}: disorder: {}\n", files[0], i + 1, lines[i])
                            }
                            _ => String::new(),
                        };
                        return Ok(ExecResult::err(stderr, 1));
                    }
                }
                return Ok(ExecResult::ok(String::new()));
            }
        }

        let mut all_lines: Vec<String> = if merge {
            // k-way merge of pre-sorted inputs; ties go to the earlier input.
            let mut idx = vec![0usize; streams.len()];
            let mut merged = Vec::new();
            loop {
                let mut best: Option<usize> = None;
                for (s, stream) in streams.iter().enumerate() {
                    if idx[s] >= stream.len() {
                        continue;
                    }
                    best = match best {
                        Some(b)
                            if cfg.compare(&stream[idx[s]], &streams[b][idx[b]])
                                != Ordering::Less =>
                        {
                            Some(b)
                        }
                        _ => Some(s),
                    };
                }
                let Some(b) = best else { break };
                merged.push(std::mem::take(&mut streams[b][idx[b]]));
                idx[b] += 1;
            }
            merged
        } else {
            let mut v = streams.concat();
            // Stable so `-u` keeps the first input line of each equal run.
            v.sort_by(|a, b| cfg.compare(a, b));
            v
        };

        if unique {
            let mut out: Vec<String> = Vec::with_capacity(all_lines.len());
            for line in all_lines.drain(..) {
                if out
                    .last()
                    .is_none_or(|prev| cfg.compare(prev, &line) != Ordering::Equal)
                {
                    out.push(line);
                }
            }
            all_lines = out;
        }

        let sep = if zero_terminated { "\0" } else { "\n" };
        let mut output = all_lines.join(sep);
        if !all_lines.is_empty() {
            output.push_str(sep);
        }

        // Write to output file if -o specified
        if let Some(ref outfile) = output_file {
            let path = if outfile.starts_with('/') {
                std::path::PathBuf::from(outfile)
            } else {
                vfs_join(ctx.cwd, outfile)
            };
            if let Err(e) = ctx.fs.write_file(&path, output.as_bytes()).await {
                return Ok(sort_err(format!("sort: {}: {}\n", outfile, e)));
            }
            return Ok(ExecResult::ok(String::new()));
        }

        Ok(ExecResult::ok(output))
    }
}

/// The uniq builtin - report or omit repeated lines.
///
/// Usage: uniq [-cdDiuz] [-f N] [-s N] [-w N] [INPUT [OUTPUT]]
///
/// Options:
///   -c   Prefix lines by the number of occurrences
///   -d   Only print duplicate lines, one for each group
///   -D   Print all duplicate lines
///   -u   Only print unique lines
///   -i   Case insensitive comparison
///   -f N Skip N fields before comparing
///   -s N Skip N characters before comparing
///   -w N Compare no more than N characters
///   -z   Line delimiter is NUL, not newline
pub struct Uniq;

#[derive(Default)]
struct UniqKeyOpts {
    skip_fields: usize,
    skip_chars: usize,
    check_chars: Option<usize>,
    case_insensitive: bool,
}

/// Get the comparison key for a line: GNU skips N blank-then-nonblank
/// fields, then N characters, then compares at most `-w` characters.
fn uniq_key(line: &str, o: &UniqKeyOpts) -> String {
    let is_blank = |c: char| c == ' ' || c == '\t';
    let mut rest = line;
    for _ in 0..o.skip_fields {
        rest = rest.trim_start_matches(is_blank);
        rest = rest.trim_start_matches(|c| !is_blank(c));
    }
    let start = advance_chars(rest, 0, o.skip_chars);
    let rest = &rest[start..];
    let end = o
        .check_chars
        .map(|n| advance_chars(rest, 0, n))
        .unwrap_or(rest.len());
    let key = &rest[..end];
    if o.case_insensitive {
        key.to_lowercase()
    } else {
        key.to_string()
    }
}

#[allow(clippy::result_large_err)]
fn parse_uniq_count(opt: &str, val: &str) -> std::result::Result<usize, ExecResult> {
    val.trim().parse().map_err(|_| {
        let what = match opt {
            "f" => "number of fields to skip",
            "s" => "number of bytes to skip",
            _ => "number of bytes to compare",
        };
        ExecResult::err(format!("uniq: {val}: invalid {what}\n"), 1)
    })
}

#[async_trait]
impl Builtin for Uniq {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: uniq [OPTION]... [INPUT [OUTPUT]]\nFilter adjacent matching lines from INPUT, writing to OUTPUT.\n\n  -c\t\tprefix lines by the number of occurrences\n  -d\t\tonly print duplicate lines, one for each group\n  -D\t\tprint all duplicate lines\n  -u\t\tonly print unique lines\n  -i\t\tignore differences in case when comparing\n  -f NUM\tavoid comparing the first NUM fields\n  -s NUM\tavoid comparing the first NUM characters\n  -w NUM\tcompare no more than NUM characters in lines\n  -z\t\tline delimiter is NUL, not newline\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("uniq (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let mut count = false;
        let mut only_duplicates = false;
        let mut all_duplicates = false;
        let mut only_unique = false;
        let mut zero_terminated = false;
        let mut key_opts = UniqKeyOpts::default();
        let mut files = Vec::new();

        let args = ctx.args;
        let mut i = 0;
        let mut opts_done = false;
        while i < args.len() {
            let arg = args[i].as_str();
            i += 1;
            if opts_done || arg == "-" || !arg.starts_with('-') {
                files.push(arg.to_string());
                continue;
            }
            if arg == "--" {
                opts_done = true;
                continue;
            }
            if let Some(long) = arg.strip_prefix("--") {
                let (name, inline) = match long.split_once('=') {
                    Some((n, v)) => (n, Some(v.to_string())),
                    None => (long, None),
                };
                let short = match name {
                    "count" => 'c',
                    "repeated" => 'd',
                    "all-repeated" => 'D',
                    "unique" => 'u',
                    "ignore-case" => 'i',
                    "zero-terminated" => 'z',
                    "skip-fields" => 'f',
                    "skip-chars" => 's',
                    "check-chars" => 'w',
                    _ => return Ok(super::invalid_option("uniq", arg, 1)),
                };
                if matches!(short, 'f' | 's' | 'w') {
                    let val = match inline {
                        Some(v) => v,
                        None if i < args.len() => {
                            i += 1;
                            args[i - 1].clone()
                        }
                        None => {
                            return Ok(ExecResult::err(
                                format!("uniq: option '--{name}' requires an argument\n"),
                                1,
                            ));
                        }
                    };
                    let n = match parse_uniq_count(&short.to_string(), &val) {
                        Ok(n) => n,
                        Err(e) => return Ok(e),
                    };
                    match short {
                        'f' => key_opts.skip_fields = n,
                        's' => key_opts.skip_chars = n,
                        _ => key_opts.check_chars = Some(n),
                    }
                    continue;
                }
                match short {
                    'c' => count = true,
                    'd' => only_duplicates = true,
                    'D' => all_duplicates = true,
                    'u' => only_unique = true,
                    'i' => key_opts.case_insensitive = true,
                    _ => zero_terminated = true,
                }
                continue;
            }
            let body = &arg[1..];
            for (pos, c) in body.char_indices() {
                match c {
                    'f' | 's' | 'w' => {
                        let attached = &body[pos + 1..];
                        let val = if !attached.is_empty() {
                            attached.to_string()
                        } else if i < args.len() {
                            i += 1;
                            args[i - 1].clone()
                        } else {
                            return Ok(ExecResult::err(
                                format!("uniq: option requires an argument -- '{c}'\n"),
                                1,
                            ));
                        };
                        let n = match parse_uniq_count(&c.to_string(), &val) {
                            Ok(n) => n,
                            Err(e) => return Ok(e),
                        };
                        match c {
                            'f' => key_opts.skip_fields = n,
                            's' => key_opts.skip_chars = n,
                            _ => key_opts.check_chars = Some(n),
                        }
                        break;
                    }
                    'c' => count = true,
                    'd' => only_duplicates = true,
                    'D' => all_duplicates = true,
                    'u' => only_unique = true,
                    'i' => key_opts.case_insensitive = true,
                    'z' => zero_terminated = true,
                    _ => return Ok(super::invalid_option("uniq", &format!("-{c}"), 1)),
                }
            }
        }

        let sep = if zero_terminated { '\0' } else { '\n' };
        let text = match files.first().map(String::as_str) {
            None | Some("-") => ctx.stdin.map(|s| s.to_string()).unwrap_or_default(),
            Some(file) => {
                let path = if file.starts_with('/') {
                    std::path::PathBuf::from(file)
                } else {
                    vfs_join(ctx.cwd, file)
                };
                match read_text_file(&*ctx.fs, &path, "uniq").await {
                    Ok(text) => text,
                    Err(e) => return Ok(e),
                }
            }
        };
        let lines: Vec<&str> = split_records(&text, sep).collect();

        // Group adjacent lines with equal keys.
        let mut groups: Vec<(usize, usize)> = Vec::new(); // (start, len)
        let mut prev_key: Option<String> = None;
        for (idx, line) in lines.iter().enumerate() {
            let key = uniq_key(line, &key_opts);
            match (&prev_key, groups.last_mut()) {
                (Some(pk), Some(g)) if *pk == key => g.1 += 1,
                _ => groups.push((idx, 1)),
            }
            prev_key = Some(key);
        }

        let mut output = String::new();
        let mut emit = |line: &str, n: usize| {
            if count {
                output.push_str(&format!("{:>7} ", n));
            }
            output.push_str(line);
            output.push(sep);
        };
        for &(start, n) in &groups {
            if all_duplicates {
                if n > 1 {
                    for line in &lines[start..start + n] {
                        emit(line, n);
                    }
                }
                continue;
            }
            let keep = if only_duplicates && only_unique {
                false
            } else if only_duplicates {
                n > 1
            } else if only_unique {
                n == 1
            } else {
                true
            };
            if keep {
                emit(lines[start], n);
            }
        }

        if let Some(out) = files.get(1) {
            let path = if out.starts_with('/') {
                std::path::PathBuf::from(out)
            } else {
                vfs_join(ctx.cwd, out)
            };
            if let Err(e) = ctx.fs.write_file(&path, output.as_bytes()).await {
                return Ok(ExecResult::err(format!("uniq: {out}: {e}\n"), 1));
            }
            return Ok(ExecResult::ok(String::new()));
        }

        Ok(ExecResult::ok(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_sort(args: &[&str], stdin: Option<&str>) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");

        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin: crate::builtins::test_stream_opt(stdin),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        Sort.execute(ctx).await.unwrap()
    }

    async fn run_uniq(args: &[&str], stdin: Option<&str>) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");

        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin: crate::builtins::test_stream_opt(stdin),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        Uniq.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_sort_basic() {
        let result = run_sort(&[], Some("banana\napple\ncherry\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "apple\nbanana\ncherry\n");
    }

    #[tokio::test]
    async fn test_sort_reverse() {
        let result = run_sort(&["-r"], Some("apple\nbanana\ncherry\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "cherry\nbanana\napple\n");
    }

    #[tokio::test]
    async fn test_sort_numeric() {
        let result = run_sort(&["-n"], Some("10\n2\n1\n20\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1\n2\n10\n20\n");
    }

    #[tokio::test]
    async fn test_sort_unique() {
        let result = run_sort(&["-u"], Some("apple\nbanana\napple\ncherry\nbanana\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "apple\nbanana\ncherry\n");
    }

    #[tokio::test]
    async fn test_sort_fold_case() {
        let result = run_sort(&["-f"], Some("Banana\napple\nCherry\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "apple\nBanana\nCherry\n");
    }

    #[tokio::test]
    async fn test_uniq_basic() {
        let result = run_uniq(&[], Some("a\na\nb\nb\nb\nc\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\nb\nc\n");
    }

    #[tokio::test]
    async fn test_uniq_count() {
        let result = run_uniq(&["-c"], Some("a\na\nb\nc\nc\nc\n")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("2 a"));
        assert!(result.stdout.contains("1 b"));
        assert!(result.stdout.contains("3 c"));
    }

    #[tokio::test]
    async fn test_uniq_duplicates_only() {
        let result = run_uniq(&["-d"], Some("a\na\nb\nc\nc\n")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("a"));
        assert!(result.stdout.contains("c"));
        assert!(!result.stdout.contains("b\n"));
    }

    #[tokio::test]
    async fn test_uniq_unique_only() {
        let result = run_uniq(&["-u"], Some("a\na\nb\nc\nc\n")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("b"));
        assert!(!result.stdout.contains("a\n"));
        assert!(!result.stdout.contains("c\n"));
    }

    #[tokio::test]
    async fn test_sort_key_field() {
        let result = run_sort(&["-k2n"], Some("Bob 25\nAlice 30\nDavid 20\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "David 20\nBob 25\nAlice 30\n");
    }

    #[tokio::test]
    async fn test_sort_delimiter_key() {
        let result = run_sort(&["-t:", "-k2n"], Some("b:2\na:1\nc:3\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a:1\nb:2\nc:3\n");
    }

    #[tokio::test]
    async fn test_sort_check_sorted() {
        let result = run_sort(&["-c"], Some("a\nb\nc\n")).await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_sort_check_unsorted() {
        let result = run_sort(&["-c"], Some("b\na\nc\n")).await;
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_sort_human_numeric() {
        let result = run_sort(&["-h"], Some("10K\n1K\n100M\n1G\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1K\n10K\n100M\n1G\n");
    }

    #[tokio::test]
    async fn test_sort_month() {
        let result = run_sort(&["-M"], Some("Mar\nJan\nFeb\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "Jan\nFeb\nMar\n");
    }

    #[tokio::test]
    async fn test_uniq_case_insensitive() {
        let result = run_uniq(&["-i"], Some("a\nA\nb\nB\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\nb\n");
    }

    #[tokio::test]
    async fn test_uniq_skip_fields() {
        let result = run_uniq(&["-f1"], Some("x a\ny a\nx b\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "x a\nx b\n");
    }

    fn key(spec: &str, line: &str, tab: Option<char>) -> String {
        let k = KeySpec::parse(spec).unwrap();
        String::from_utf8(key_text(line, tab, &k).to_vec()).unwrap()
    }

    #[test]
    fn test_key_text() {
        assert_eq!(key("2,2", "a:b:c", Some(':')), "b");
        assert_eq!(key("2", "a:b:c", Some(':')), "b:c");
        assert_eq!(key("1,1", "hello world", None), "hello");
        // Fields keep their leading blanks unless -b; no end = end of line.
        assert_eq!(key("2", "hello  world x", None), "  world x");
        assert_eq!(key("2b,2", "hello  world x", None), "world");
        assert_eq!(key("5", "x", None), "");
        // Char offsets may run past the field end into the rest of the line.
        assert_eq!(key("1.3", "a b", None), "b");
        // Char positions count UTF-8 characters.
        assert_eq!(key("1.2,1.2", "éa", None), "a");
        assert_eq!(key("1.2,2.1", "éx yz", None), "x ");
    }

    #[test]
    fn test_keyspec_parse() {
        let k = KeySpec::parse("2n,3r").unwrap();
        assert_eq!((k.sword, k.eword), (1, Some(2)));
        assert!(k.opts.numeric && k.opts.reverse);

        let k2 = KeySpec::parse("1.2,1.5f").unwrap();
        assert_eq!((k2.sword, k2.schar, k2.eword, k2.echar), (0, 1, Some(0), 5));
        assert!(k2.opts.fold);

        assert!(KeySpec::parse("0").is_err());
        assert!(KeySpec::parse("1.0").is_err());
        assert!(KeySpec::parse("1x").is_err());
    }

    #[test]
    fn test_version_cmp() {
        assert_eq!(version_cmp("1.2", "1.10"), Ordering::Less);
        assert_eq!(version_cmp("2.0", "1.9"), Ordering::Greater);
        assert_eq!(version_cmp("1.0", "1.0"), Ordering::Equal);
        assert_eq!(version_cmp("a-1.9~rc", "a-1.9"), Ordering::Less);
        assert_eq!(version_cmp(".", ".."), Ordering::Less);
        assert_eq!(version_cmp("file.tar.gz", "file2.tar.gz"), Ordering::Less);
    }

    #[test]
    fn test_numeric_compare_is_exact() {
        assert_eq!(
            numcompare(b"100000000000000000001", b"100000000000000000000"),
            Ordering::Greater
        );
        assert_eq!(numcompare(b"-0", b"0"), Ordering::Equal);
        assert_eq!(numcompare(b"+5", b"0"), Ordering::Equal);
        assert_eq!(numcompare(b".5", b"0.50"), Ordering::Equal);
        assert_eq!(numcompare(b"-2", b"-10"), Ordering::Greater);
    }

    #[test]
    fn test_human_and_general_compare() {
        assert_eq!(human_compare(b"1023K", b"1M"), Ordering::Less);
        assert_eq!(human_compare(b"-1K", b"-2"), Ordering::Less);
        assert_eq!(general_compare(b"abc", b"nan"), Ordering::Less);
        assert_eq!(general_compare(b"0x10", b"1e3"), Ordering::Less);
        assert_eq!(general_compare(b"10", b"0x10"), Ordering::Less);
    }

    #[test]
    fn test_month_ordinal() {
        assert_eq!(month_ordinal(b"JAN"), 1);
        assert_eq!(month_ordinal(b"  feb"), 2);
        assert_eq!(month_ordinal(b"december"), 12);
        assert_eq!(month_ordinal(b"xyz"), 0);
    }

    #[tokio::test]
    async fn test_sort_key_options_override_global() {
        let result = run_sort(&["-r", "-k2n"], Some("a 2 z\nb 10 a\nc 2 b\n")).await;
        assert_eq!(result.stdout, "c 2 b\na 2 z\nb 10 a\n");
    }

    #[tokio::test]
    async fn test_sort_unique_uses_keys_only() {
        let result = run_sort(&["-nu"], Some("1\n01\nb\na\n")).await;
        assert_eq!(result.stdout, "b\n1\n");
    }

    #[tokio::test]
    async fn test_sort_bad_key_is_error() {
        let result = run_sort(&["-k0"], Some("a\n")).await;
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("field number is zero"));
    }

    #[tokio::test]
    async fn test_sort_rejects_unknown_option() {
        let result = run_sort(&["-Q"], Some("a\n")).await;
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("invalid option -- 'Q'"));
    }

    #[tokio::test]
    async fn test_uniq_rejects_unknown_option() {
        let result = run_uniq(&["-Q"], Some("a\n")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option -- 'Q'"));
    }
}
