//! `find -printf` formatting (GNU findutils directives, escapes, and
//! `%[-][width][.precision]X` field padding).
//!
//! Values with no VFS counterpart are fixed: `%i` (inode) and `%D` (device)
//! print 0, `%F` prints `vfs`, `%S` prints 1, `%Z` prints `?`.

use super::{Entry, FmtEnv, nanos_of};
use crate::fs::Metadata;

fn type_char(meta: &Metadata) -> char {
    if meta.file_type.is_dir() {
        'd'
    } else if meta.file_type.is_symlink() {
        'l'
    } else if matches!(meta.file_type, crate::fs::FileType::Fifo) {
        'p'
    } else {
        'f'
    }
}

pub(super) fn symbolic_mode(meta: &Metadata) -> String {
    let mut s = String::with_capacity(10);
    s.push(match type_char(meta) {
        'f' => '-',
        c => c,
    });
    let mode = meta.mode;
    for (shift, special, set_char) in [(6u32, 0o4000, 's'), (3, 0o2000, 's'), (0, 0o1000, 't')] {
        let bits = (mode >> shift) & 7;
        s.push(if bits & 4 != 0 { 'r' } else { '-' });
        s.push(if bits & 2 != 0 { 'w' } else { '-' });
        let x = bits & 1 != 0;
        s.push(match (mode & special != 0, x) {
            (true, true) => set_char,
            (true, false) => set_char.to_ascii_uppercase(),
            (false, true) => 'x',
            (false, false) => '-',
        });
    }
    s
}

/// `%h`: leading directories (`.` when there is no slash).
fn dirname(path: &str) -> &str {
    match path.rfind('/') {
        Some(idx) => &path[..idx],
        None => ".",
    }
}

/// `%f`: last component; a trailing slash is kept, as GNU does.
fn basename(path: &str) -> String {
    if path.chars().all(|c| c == '/') && !path.is_empty() {
        return "/".to_string();
    }
    let trimmed = path.trim_end_matches('/');
    let tail = &path[trimmed.len()..];
    let base = trimmed.rsplit('/').next().unwrap_or(trimmed);
    format!("{base}{tail}")
}

/// Time directive (`%A`, `%C`, `%T` with a key char), with GNU's fractional
/// seconds on `@`, `S`, `T`, and `+`.
fn time_value(nanos: i128, key: char, env: &FmtEnv) -> String {
    let secs = i64::try_from(nanos.div_euclid(1_000_000_000)).unwrap_or(0);
    let frac = format!("{:09}0", nanos.rem_euclid(1_000_000_000));
    let fmt = |f: &str| env.strftime(secs, f);
    match key {
        '@' => format!("{secs}.{frac}"),
        'S' => format!("{}.{frac}", fmt("%S")),
        'T' => format!("{}.{frac}", fmt("%H:%M:%S")),
        '+' => format!("{}.{frac}", fmt("%Y-%m-%d+%H:%M:%S")),
        k => fmt(&format!("%{k}")),
    }
}

/// `%t`/`%a`/`%c`: ctime(3) layout with GNU's fractional seconds.
fn ctime_value(nanos: i128, env: &FmtEnv) -> String {
    let secs = i64::try_from(nanos.div_euclid(1_000_000_000)).unwrap_or(0);
    format!(
        "{}.{:09}0 {}",
        env.strftime(secs, "%a %b %e %H:%M:%S"),
        nanos.rem_euclid(1_000_000_000),
        env.strftime(secs, "%Y")
    )
}

fn blocks_1k(meta: &Metadata) -> u64 {
    if meta.file_type.is_symlink() {
        0
    } else {
        meta.size.div_ceil(4096) * 4
    }
}

fn pad(value: String, left: bool, width: Option<usize>, precision: Option<usize>) -> String {
    let mut value = value;
    if let Some(p) = precision {
        value = value.chars().take(p).collect();
    }
    match width {
        Some(w) if value.chars().count() < w => {
            let fill = " ".repeat(w - value.chars().count());
            if left { value + &fill } else { fill + &value }
        }
        _ => value,
    }
}

/// Render one `-printf` format for `entry`.
pub(super) fn render(fmt: &str, entry: &Entry, start: &str, env: &FmtEnv) -> String {
    let chars: Vec<char> = fmt.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let meta = entry.meta();
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        match c {
            '\\' => {
                let Some(&n) = chars.get(i) else {
                    out.push('\\');
                    break;
                };
                i += 1;
                match n {
                    'a' => out.push('\x07'),
                    'b' => out.push('\x08'),
                    'c' => return out,
                    'f' => out.push('\x0c'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'v' => out.push('\x0b'),
                    '\\' => out.push('\\'),
                    '0'..='7' => {
                        let mut v = n.to_digit(8).unwrap_or(0);
                        let mut digits = 1;
                        while digits < 3
                            && let Some(d) = chars.get(i).and_then(|c| c.to_digit(8))
                        {
                            v = v * 8 + d;
                            i += 1;
                            digits += 1;
                        }
                        out.push(char::from_u32(v & 0xff).unwrap_or('?'));
                    }
                    other => {
                        out.push('\\');
                        out.push(other);
                    }
                }
            }
            '%' => {
                let spec_start = i - 1;
                let mut left = false;
                while let Some(&f) = chars.get(i)
                    && matches!(f, '-' | '+' | ' ' | '#' | '0')
                {
                    left |= f == '-';
                    i += 1;
                }
                let mut width = None;
                while let Some(d) = chars.get(i).and_then(|c| c.to_digit(10)) {
                    width = Some(width.unwrap_or(0usize) * 10 + d as usize);
                    i += 1;
                }
                let mut precision = None;
                if chars.get(i) == Some(&'.') {
                    i += 1;
                    precision = Some(0usize);
                    while let Some(d) = chars.get(i).and_then(|c| c.to_digit(10)) {
                        precision = Some(precision.unwrap_or(0) * 10 + d as usize);
                        i += 1;
                    }
                }
                let Some(&d) = chars.get(i) else {
                    out.extend(&chars[spec_start..]);
                    break;
                };
                i += 1;
                let value = match d {
                    '%' => "%".to_string(),
                    'p' => entry.display.clone(),
                    'f' => basename(&entry.display),
                    'h' => dirname(&entry.display).to_string(),
                    'H' => start.to_string(),
                    'P' => {
                        if entry.display == start {
                            String::new()
                        } else {
                            entry.display[start.len().min(entry.display.len())..]
                                .trim_start_matches('/')
                                .to_string()
                        }
                    }
                    'd' => entry.depth.to_string(),
                    's' => meta.size.to_string(),
                    'k' => blocks_1k(meta).to_string(),
                    'b' => (blocks_1k(meta) * 2).to_string(),
                    'm' => format!("{:o}", meta.mode & 0o7777),
                    'M' => symbolic_mode(meta),
                    'y' => type_char(&entry.lmeta).to_string(),
                    'Y' => match (&entry.link_target, &entry.fmeta) {
                        (None, _) => type_char(&entry.lmeta).to_string(),
                        (Some(_), Some(m)) => type_char(m).to_string(),
                        (Some(_), None) => "N".to_string(),
                    },
                    'l' => entry.link_target.clone().unwrap_or_default(),
                    'n' => entry.nlink().to_string(),
                    'u' | 'g' => env.username.clone(),
                    'U' | 'G' => "1000".to_string(),
                    'i' | 'D' => "0".to_string(),
                    'F' => "vfs".to_string(),
                    'S' => "1".to_string(),
                    'Z' => "?".to_string(),
                    't' | 'a' => ctime_value(nanos_of(meta.modified), env),
                    'c' => ctime_value(nanos_of(meta.modified), env),
                    'T' | 'A' | 'C' | 'B' => {
                        let Some(&key) = chars.get(i) else {
                            out.extend(&chars[spec_start..]);
                            break;
                        };
                        i += 1;
                        let t = if d == 'B' {
                            nanos_of(meta.created)
                        } else {
                            nanos_of(meta.modified)
                        };
                        time_value(t, key, env)
                    }
                    _ => {
                        // Unknown directive: emitted verbatim.
                        out.extend(&chars[spec_start..i]);
                        continue;
                    }
                };
                out.push_str(&pad(value, left, width, precision));
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_parts_match_gnu() {
        assert_eq!(dirname("d"), ".");
        assert_eq!(dirname("/"), "");
        assert_eq!(dirname("./d"), ".");
        assert_eq!(dirname("d/"), "d");
        assert_eq!(basename("d/f"), "f");
        assert_eq!(basename("d/"), "d/");
        assert_eq!(basename("/"), "/");
        assert_eq!(basename("."), ".");
    }

    #[test]
    fn padding() {
        assert_eq!(pad("3".into(), true, Some(6), None), "3     ");
        assert_eq!(pad("3".into(), false, Some(6), None), "     3");
        assert_eq!(pad("abc".into(), false, None, Some(2)), "ab");
    }
}
