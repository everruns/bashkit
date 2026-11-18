//! iconv builtin - character encoding conversion (virtual)
//!
//! Converts bytes between character encodings. Input and output are raw
//! bytes end to end: `-t latin1` writes Latin-1 bytes, `-t utf-16` writes
//! real UTF-16 code units. iconv is the step that turns non-UTF-8 files into
//! text the other builtins can read (L-STREAM-001), so it must never go
//! through a lossy UTF-8 string.
//!
//! Decisions:
//! - Conversion runs char by char (decode one char, encode it), so error
//!   positions are byte offsets in the input, like GNU iconv.
//! - On a hard error the converted prefix is still written to stdout and the
//!   exit status is 1 (GNU). With `-o FILE` nothing is written unless the
//!   whole input converted, so a wrong `-f` guess never damages FILE, even
//!   when FILE is also the input.
//! - `-c` / `//IGNORE` drop invalid input and characters the target cannot
//!   hold, write the rest, and exit 1 (POSIX, GNU libiconv). glibc 2.39 exits
//!   0 when only unconvertible characters were dropped; we follow POSIX.
//! - `UTF-16` output starts with a BOM and is little-endian; `UTF-16LE` and
//!   `UTF-16BE` write no BOM. `UTF-16` input honours a BOM and defaults to
//!   little-endian without one (glibc on little-endian hosts).
//! - Output growth (up to 2x for UTF-16) is charged to the shared execution
//!   budget (TM-DOS-103 pattern used by `od`).
//!
//! Usage:
//!   iconv -f UTF-8 -t LATIN1 file.txt
//!   iconv -f LATIN1 -t UTF-8 -o file.txt file.txt
//!   echo "hello" | iconv -f UTF-8 -t ASCII//TRANSLIT
//!   iconv -l

use async_trait::async_trait;

use super::{Builtin, BuiltinHelper, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;
use crate::limits::{BudgetedBytes, ExecutionBudget};

/// iconv builtin - character encoding conversion.
pub struct Iconv;

impl BuiltinHelper for Iconv {
    const NAME: &'static str = "iconv";
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Charset {
    Utf8,
    Ascii,
    Latin1,
    /// UTF-16 with BOM: BOM-detected on input, BOM + little-endian on output.
    Utf16,
    Utf16Le,
    Utf16Be,
}

/// Look up a charset name, ignoring case, `-` and `_`.
fn lookup_charset(name: &str) -> Option<Charset> {
    let key: String = name
        .chars()
        .filter(|c| !matches!(c, '-' | '_'))
        .collect::<String>()
        .to_ascii_lowercase();
    Some(match key.as_str() {
        "utf8" => Charset::Utf8,
        "ascii" | "usascii" | "ansix3.41968" | "646" => Charset::Ascii,
        "latin1" | "iso88591" | "iso885911987" | "l1" | "cp819" | "ibm819" => Charset::Latin1,
        "utf16" => Charset::Utf16,
        "utf16le" => Charset::Utf16Le,
        "utf16be" => Charset::Utf16Be,
        _ => return None,
    })
}

/// A parsed encoding argument like `ASCII//TRANSLIT//IGNORE`.
struct EncodingSpec {
    charset: Charset,
    translit: bool,
    ignore: bool,
}

/// Parse `NAME[//SUFFIX...]`. Suffixes may also be comma-separated
/// (`//TRANSLIT,IGNORE`), as glibc accepts.
fn parse_encoding_spec(spec: &str) -> Option<EncodingSpec> {
    let mut parts = spec.split("//");
    let charset = lookup_charset(parts.next()?)?;
    let mut out = EncodingSpec {
        charset,
        translit: false,
        ignore: false,
    };
    for part in parts {
        for suffix in part.split(',') {
            if suffix.eq_ignore_ascii_case("translit") {
                out.translit = true;
            } else if suffix.eq_ignore_ascii_case("ignore") {
                out.ignore = true;
            } else if !suffix.is_empty() {
                return None;
            }
        }
    }
    Some(out)
}

/// Closest ASCII spelling for a character, used by `//TRANSLIT`.
fn transliterate(ch: char) -> Option<&'static str> {
    Some(match ch {
        'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'í' | 'ì' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' => "i",
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
        'ú' | 'ù' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'ý' | 'ÿ' | 'ŷ' => "y",
        'ñ' | 'ń' | 'ņ' | 'ň' => "n",
        'ç' | 'ć' | 'č' | 'ĉ' | 'ċ' => "c",
        'ß' => "ss",
        'æ' => "ae",
        'œ' => "oe",
        'ð' | 'đ' | 'ď' => "d",
        'þ' => "th",
        'ł' | 'ľ' | 'ĺ' => "l",
        'ğ' => "g",
        'ş' | 'š' | 'ś' | 'ŝ' => "s",
        'ž' | 'ź' | 'ż' => "z",
        'ř' | 'ŕ' => "r",
        'ť' | 'ţ' => "t",
        'Á' | 'À' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'Ā' | 'Ă' | 'Ą' => "A",
        'É' | 'È' | 'Ê' | 'Ë' | 'Ē' | 'Ė' | 'Ę' | 'Ě' => "E",
        'Í' | 'Ì' | 'Î' | 'Ï' | 'Ī' | 'Į' => "I",
        'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' | 'Ø' | 'Ō' | 'Ő' => "O",
        'Ú' | 'Ù' | 'Û' | 'Ü' | 'Ū' | 'Ů' | 'Ű' | 'Ų' => "U",
        'Ý' | 'Ÿ' => "Y",
        'Ñ' | 'Ń' | 'Ň' => "N",
        'Ç' | 'Ć' | 'Č' => "C",
        'Æ' => "AE",
        'Œ' => "OE",
        'Ð' | 'Đ' | 'Ď' => "D",
        'Þ' => "TH",
        'Ł' | 'Ľ' | 'Ĺ' => "L",
        'Ğ' => "G",
        'Ş' | 'Š' | 'Ś' => "S",
        'Ž' | 'Ź' | 'Ż' => "Z",
        'Ř' | 'Ŕ' => "R",
        'Ť' | 'Ţ' => "T",
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{2032}' => "'",
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{2033}' => "\"",
        '\u{2013}' | '\u{2014}' | '\u{2212}' => "-",
        '\u{2026}' => "...",
        '\u{00A0}' => " ",
        '\u{20AC}' => "EUR",
        '\u{00A9}' => "(C)",
        '\u{00AE}' => "(R)",
        '\u{2122}' => "(TM)",
        '\u{00AB}' => "<<",
        '\u{00BB}' => ">>",
        _ => return None,
    })
}

const SUPPORTED_ENCODINGS: &[&str] = &[
    "ASCII",
    "ISO-8859-1",
    "LATIN1",
    "UTF-16",
    "UTF-16BE",
    "UTF-16LE",
    "UTF-8",
];

/// One step of decoding.
enum Decoded {
    Char(char),
    /// Invalid sequence; the returned length says how many bytes to skip.
    Invalid,
    /// Truncated sequence at the end of input.
    Incomplete,
}

/// Decode the char at `input[pos..]`, returning it and the bytes consumed.
fn decode_one(input: &[u8], pos: usize, charset: Charset, big_endian: bool) -> (Decoded, usize) {
    let rest = &input[pos..];
    match charset {
        Charset::Utf8 => {
            let b = rest[0];
            if b < 0x80 {
                return (Decoded::Char(b as char), 1);
            }
            let chunk = &rest[..rest.len().min(4)];
            match std::str::from_utf8(chunk) {
                Ok(s) => first_char(s),
                Err(e) if e.valid_up_to() > 0 => {
                    // valid_up_to() is a char boundary, so this cannot fail.
                    first_char(std::str::from_utf8(&chunk[..e.valid_up_to()]).unwrap_or("\0"))
                }
                Err(e) => match e.error_len() {
                    Some(n) => (Decoded::Invalid, n),
                    None => (Decoded::Incomplete, rest.len()),
                },
            }
        }
        Charset::Ascii => {
            let b = rest[0];
            if b < 0x80 {
                (Decoded::Char(b as char), 1)
            } else {
                (Decoded::Invalid, 1)
            }
        }
        Charset::Latin1 => (Decoded::Char(rest[0] as char), 1),
        Charset::Utf16 | Charset::Utf16Le | Charset::Utf16Be => {
            let unit = |i: usize| -> Option<u16> {
                let b = rest.get(i..i + 2)?;
                Some(if big_endian {
                    u16::from_be_bytes([b[0], b[1]])
                } else {
                    u16::from_le_bytes([b[0], b[1]])
                })
            };
            let Some(first) = unit(0) else {
                return (Decoded::Incomplete, rest.len());
            };
            match first {
                0xD800..=0xDBFF => match unit(2) {
                    None => (Decoded::Incomplete, rest.len()),
                    Some(second @ 0xDC00..=0xDFFF) => {
                        let cp = 0x10000
                            + ((u32::from(first) - 0xD800) << 10)
                            + (u32::from(second) - 0xDC00);
                        match char::from_u32(cp) {
                            Some(c) => (Decoded::Char(c), 4),
                            None => (Decoded::Invalid, 2),
                        }
                    }
                    Some(_) => (Decoded::Invalid, 2),
                },
                0xDC00..=0xDFFF => (Decoded::Invalid, 2),
                _ => match char::from_u32(u32::from(first)) {
                    Some(c) => (Decoded::Char(c), 2),
                    None => (Decoded::Invalid, 2),
                },
            }
        }
    }
}

fn first_char(s: &str) -> (Decoded, usize) {
    match s.chars().next() {
        Some(c) => (Decoded::Char(c), c.len_utf8()),
        None => (Decoded::Invalid, 1),
    }
}

/// Encode one char. Returns false when the target cannot hold it.
fn encode_one(
    ch: char,
    charset: Charset,
    out: &mut BudgetedBytes,
) -> std::result::Result<bool, crate::limits::LimitExceeded> {
    let mut buf = [0u8; 4];
    match charset {
        Charset::Utf8 => out.try_extend_from_slice(ch.encode_utf8(&mut buf).as_bytes())?,
        Charset::Ascii if ch.is_ascii() => out.try_push(ch as u8)?,
        Charset::Latin1 if (ch as u32) < 0x100 => out.try_push(ch as u32 as u8)?,
        Charset::Ascii | Charset::Latin1 => return Ok(false),
        Charset::Utf16 | Charset::Utf16Le | Charset::Utf16Be => {
            let mut units = [0u16; 2];
            for u in ch.encode_utf16(&mut units) {
                let bytes = if charset == Charset::Utf16Be {
                    u.to_be_bytes()
                } else {
                    u.to_le_bytes()
                };
                out.try_extend_from_slice(&bytes)?;
            }
        }
    }
    Ok(true)
}

/// Why a conversion stopped early.
enum ConvertError {
    Illegal(usize),
    Incomplete,
}

impl ConvertError {
    fn message(&self) -> String {
        match self {
            Self::Illegal(pos) => format!("iconv: illegal input sequence at position {pos}\n"),
            Self::Incomplete => {
                "iconv: incomplete character or shift sequence at end of buffer\n".to_string()
            }
        }
    }
}

struct Converter {
    from: Charset,
    to: Charset,
    translit: bool,
    /// `-c` or `//IGNORE`: drop what cannot be converted.
    skip: bool,
    /// Set when `skip` dropped anything (exit status 1).
    skipped: bool,
    bom_written: bool,
}

impl Converter {
    /// Convert one input file, appending to `out`.
    fn convert(
        &mut self,
        input: &[u8],
        out: &mut BudgetedBytes,
        budget: Option<&ExecutionBudget>,
    ) -> Result<std::result::Result<(), ConvertError>> {
        if self.to == Charset::Utf16 && !self.bom_written {
            out.try_extend_from_slice(&[0xFF, 0xFE])?;
            self.bom_written = true;
        }
        let mut pos = 0;
        let mut big_endian = self.from == Charset::Utf16Be;
        if self.from == Charset::Utf16 && input.len() >= 2 {
            match (input[0], input[1]) {
                (0xFF, 0xFE) => pos = 2,
                (0xFE, 0xFF) => {
                    pos = 2;
                    big_endian = true;
                }
                _ => {}
            }
        }
        if let Some(budget) = budget {
            budget.consume_work(input.len() as u64)?;
        }
        while pos < input.len() {
            let (decoded, len) = decode_one(input, pos, self.from, big_endian);
            match decoded {
                Decoded::Char(ch) => {
                    if !self.emit(ch, out)? {
                        if self.skip {
                            self.skipped = true;
                        } else {
                            return Ok(Err(ConvertError::Illegal(pos)));
                        }
                    }
                }
                Decoded::Invalid if self.skip => self.skipped = true,
                Decoded::Invalid => return Ok(Err(ConvertError::Illegal(pos))),
                // `-c` keeps what converted from a file cut mid-character.
                Decoded::Incomplete if self.skip => self.skipped = true,
                Decoded::Incomplete => return Ok(Err(ConvertError::Incomplete)),
            }
            pos += len;
        }
        Ok(Ok(()))
    }

    /// Encode `ch`, transliterating when asked. False when it cannot be written.
    fn emit(
        &self,
        ch: char,
        out: &mut BudgetedBytes,
    ) -> std::result::Result<bool, crate::limits::LimitExceeded> {
        if encode_one(ch, self.to, out)? {
            return Ok(true);
        }
        if !self.translit {
            return Ok(false);
        }
        let mark = out.len();
        if let Some(rep) = transliterate(ch) {
            let mut ok = true;
            for c in rep.chars() {
                ok &= encode_one(c, self.to, out)?;
            }
            if ok {
                return Ok(true);
            }
            out.truncate(mark);
        }
        encode_one('?', self.to, out)
    }
}

const USAGE: &str = "Usage: iconv [OPTION...] [FILE...]\nConvert encoding of given files from one encoding to another.\n\n  -f, --from-code=NAME\tencoding of original text\n  -t, --to-code=NAME\tencoding for output (suffixes //TRANSLIT, //IGNORE)\n  -c\t\t\tomit invalid characters from output\n  -o, --output=FILE\toutput file, written only when conversion finishes\n  -s, --silent\t\tsuppress warnings\n  -l, --list\t\tlist all known coded character sets\n      --help\t\tdisplay this help and exit\n      --version\t\toutput version information and exit\n";

#[async_trait]
impl Builtin for Iconv {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = Self::check_help(ctx.args, USAGE, Some("iconv (bashkit) 0.2")) {
            return Ok(r);
        }
        let mut from_enc: Option<String> = None;
        let mut to_enc: Option<String> = None;
        let mut output: Option<String> = None;
        let mut files: Vec<String> = Vec::new();
        let mut list = false;
        let mut omit_invalid = false;

        let args = ctx.args;
        let mut i = 0;
        let mut options_done = false;
        while i < args.len() {
            let arg = args[i].as_str();
            i += 1;
            if options_done || arg == "-" || !arg.starts_with('-') {
                files.push(arg.to_string());
                continue;
            }
            // Options that take a value: -f X, -fX, --from-code=X, --from-code X.
            let valued = [
                ("-f", "--from-code"),
                ("-t", "--to-code"),
                ("-o", "--output"),
            ];
            let mut matched = None;
            for (idx, (short, long)) in valued.iter().enumerate() {
                let value = if arg == *short || arg == *long {
                    match args.get(i) {
                        Some(v) => {
                            i += 1;
                            Some(v.clone())
                        }
                        None => {
                            return Ok(Self::err(
                                format!("option '{arg}' requires an argument"),
                                1,
                            ));
                        }
                    }
                } else if let Some(v) = arg.strip_prefix(&format!("{long}=")) {
                    Some(v.to_string())
                } else if arg.len() > 2 && arg.starts_with(short) && !arg.starts_with("--") {
                    Some(arg[2..].to_string())
                } else {
                    None
                };
                if let Some(v) = value {
                    matched = Some((idx, v));
                    break;
                }
            }
            if let Some((idx, v)) = matched {
                match idx {
                    0 => from_enc = Some(v),
                    1 => to_enc = Some(v),
                    _ => output = Some(v),
                }
                continue;
            }
            match arg {
                "--" => options_done = true,
                "-l" | "--list" => list = true,
                "-c" => omit_invalid = true,
                "-s" | "--silent" => {}
                _ if arg.len() > 1
                    && !arg.starts_with("--")
                    && arg[1..].chars().all(|c| matches!(c, 'c' | 's' | 'l')) =>
                {
                    omit_invalid |= arg.contains('c');
                    list |= arg.contains('l');
                }
                _ => return Ok(Self::err(format!("unknown option '{arg}'"), 1)),
            }
        }

        if list {
            let mut out = String::new();
            for enc in SUPPORTED_ENCODINGS {
                out.push_str(enc);
                out.push('\n');
            }
            return Ok(ExecResult::ok(out));
        }

        let from = match &from_enc {
            Some(f) => match parse_encoding_spec(f) {
                Some(spec) => spec.charset,
                None => return Ok(Self::err(format!("unsupported encoding '{f}'"), 1)),
            },
            None => return Ok(Self::err("missing source encoding (-f)", 1)),
        };
        let to = match &to_enc {
            Some(t) => match parse_encoding_spec(t) {
                Some(spec) => spec,
                None => return Ok(Self::err(format!("unsupported encoding '{t}'"), 1)),
            },
            None => return Ok(Self::err("missing target encoding (-t)", 1)),
        };

        if files.is_empty() {
            if ctx.stdin.is_none() {
                return Ok(Self::err(
                    "no input (provide file argument or pipe stdin)",
                    1,
                ));
            }
            files.push("-".to_string());
        }

        let budget = ctx
            .execution_budget()
            .map(|budget| {
                budget
                    .try_with(Clone::clone)
                    .map_err(|_| crate::Error::Cancelled)
            })
            .transpose()?;
        let mut out = BudgetedBytes::new(budget.as_ref())?;
        let mut converter = Converter {
            from,
            to: to.charset,
            translit: to.translit,
            skip: omit_invalid || to.ignore,
            skipped: false,
            bom_written: false,
        };

        let mut failure: Option<String> = None;
        for file in &files {
            let input: Vec<u8> = if file == "-" {
                ctx.stdin_bytes().unwrap_or_default().to_vec()
            } else {
                let path = resolve_path(ctx.cwd, file);
                match ctx.fs.read_file(&path).await {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        failure = Some(format!(
                            "iconv: {file}: {}\n",
                            crate::error::io_error_reason(&e)
                        ));
                        break;
                    }
                }
            };
            if let Err(e) = converter.convert(&input, &mut out, budget.as_ref())? {
                failure = Some(e.message());
                break;
            }
        }

        let (bytes, _lease) = out.into_parts();
        let exit_code = i32::from(failure.is_some() || converter.skipped);

        if let Some(target) = output {
            // Only a complete conversion replaces the file.
            if let Some(msg) = failure {
                return Ok(ExecResult::err(msg, 1));
            }
            let path = resolve_path(ctx.cwd, &target);
            if let Err(e) = ctx.fs.write_file(&path, &bytes).await {
                return Ok(Self::err_path(
                    &target,
                    crate::error::io_error_reason(&e),
                    1,
                ));
            }
            return Ok(ExecResult::with_code(String::new(), exit_code));
        }

        let mut result = ExecResult::ok_bytes(bytes);
        result.exit_code = exit_code;
        if let Some(msg) = failure {
            result.stderr = msg.into();
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    async fn run_bytes(args: &[&str], stdin: Option<&[u8]>, fs: Arc<InMemoryFs>) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let fs_dyn = fs as Arc<dyn FileSystem>;
        let stdin: Option<&'static crate::StreamData> =
            stdin.map(|b| &*Box::leak(Box::new(crate::StreamData::from(b.to_vec()))));
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs_dyn,
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };
        Iconv.execute(ctx).await.unwrap()
    }

    async fn run(args: &[&str], stdin: Option<&str>) -> ExecResult {
        run_bytes(args, stdin.map(str::as_bytes), Arc::new(InMemoryFs::new())).await
    }

    async fn fs_with(path: &str, bytes: &[u8]) -> Arc<InMemoryFs> {
        let fs = Arc::new(InMemoryFs::new());
        fs.write_file(Path::new(path), bytes).await.unwrap();
        fs
    }

    // --- positive ---

    #[tokio::test]
    async fn list_encodings() {
        let r = run(&["-l"], None).await;
        assert_eq!(r.exit_code, 0);
        for name in ["UTF-8", "ASCII", "LATIN1", "UTF-16LE", "UTF-16BE"] {
            assert!(r.stdout.contains(name), "{name}");
        }
    }

    #[tokio::test]
    async fn utf8_to_ascii() {
        let r = run(&["-f", "UTF-8", "-t", "ASCII"], Some("hello")).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hello");
    }

    #[tokio::test]
    async fn utf8_to_latin1_writes_latin1_bytes() {
        let r = run(&["-f", "UTF-8", "-t", "LATIN1"], Some("caf\u{e9}")).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.as_bytes(), b"caf\xe9");
    }

    #[tokio::test]
    async fn repairs_double_encoded_utf8() {
        // "Köln" encoded as UTF-8, then read as Latin-1 and encoded again.
        let r = run(&["-f", "utf-8", "-t", "latin1"], Some("K\u{c3}\u{b6}ln")).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.as_bytes(), "Köln".as_bytes());
    }

    #[tokio::test]
    async fn latin1_file_to_utf8() {
        let fs = fs_with("/test.txt", b"caf\xe9").await;
        let r = run_bytes(&["-f", "LATIN1", "-t", "UTF-8", "/test.txt"], None, fs).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "caf\u{e9}");
    }

    #[tokio::test]
    async fn latin1_stdin_bytes_to_utf8() {
        let fs = Arc::new(InMemoryFs::new());
        let r = run_bytes(&["-f", "latin1", "-t", "utf-8"], Some(b"K\xf6ln"), fs).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "Köln");
    }

    #[tokio::test]
    async fn utf16_output_has_bom_and_le_units() {
        let r = run(&["-f", "utf-8", "-t", "utf-16"], Some("aé€")).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(
            r.stdout.as_bytes(),
            &[0xFF, 0xFE, b'a', 0, 0xE9, 0, 0xAC, 0x20]
        );
    }

    #[tokio::test]
    async fn utf16le_and_be_output_have_no_bom() {
        let r = run(&["-f", "utf-8", "-t", "UTF-16LE"], Some("ab")).await;
        assert_eq!(r.stdout.as_bytes(), b"a\0b\0");
        let r = run(&["-f", "utf-8", "-t", "UTF-16BE"], Some("ab")).await;
        assert_eq!(r.stdout.as_bytes(), b"\0a\0b");
    }

    #[tokio::test]
    async fn utf16_surrogate_pair_round_trip() {
        let r = run(&["-f", "utf-8", "-t", "utf-16be"], Some("😀")).await;
        assert_eq!(r.stdout.as_bytes(), &[0xD8, 0x3D, 0xDE, 0x00]);
        let fs = Arc::new(InMemoryFs::new());
        let r = run_bytes(
            &["-f", "utf-16be", "-t", "utf-8"],
            Some(&[0xD8, 0x3D, 0xDE, 0x00]),
            fs,
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "😀");
    }

    #[tokio::test]
    async fn utf16_input_honours_bom() {
        let fs = Arc::new(InMemoryFs::new());
        let be = run_bytes(
            &["-f", "utf-16", "-t", "utf-8"],
            Some(b"\xFE\xFF\0h\0i"),
            fs.clone(),
        )
        .await;
        assert_eq!(be.stdout, "hi");
        let le = run_bytes(
            &["-f", "utf-16", "-t", "utf-8"],
            Some(b"\xFF\xFEh\0i\0"),
            fs.clone(),
        )
        .await;
        assert_eq!(le.stdout, "hi");
        let none = run_bytes(&["-f", "utf-16", "-t", "utf-8"], Some(b"h\0i\0"), fs).await;
        assert_eq!(none.stdout, "hi");
    }

    #[tokio::test]
    async fn utf16le_input_keeps_bom_as_character() {
        let fs = Arc::new(InMemoryFs::new());
        let r = run_bytes(&["-f", "utf-16le", "-t", "utf-8"], Some(b"\xFF\xFEa\0"), fs).await;
        assert_eq!(r.stdout, "\u{FEFF}a");
    }

    #[tokio::test]
    async fn identity_conversion() {
        let r = run(&["-f", "UTF-8", "-t", "UTF-8"], Some("hello world\n")).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hello world\n");
    }

    #[tokio::test]
    async fn attached_and_long_option_forms() {
        for args in [
            &["-futf-8", "-tlatin1"][..],
            &["--from-code=utf-8", "--to-code=latin1"][..],
            &["--from-code", "utf-8", "--to-code", "latin1"][..],
        ] {
            let r = run(args, Some("é")).await;
            assert_eq!(r.exit_code, 0, "{}", args.join(" "));
            assert_eq!(r.stdout.as_bytes(), b"\xe9", "{}", args.join(" "));
        }
    }

    #[tokio::test]
    async fn multiple_files_are_concatenated() {
        let fs = fs_with("/a", b"\xe9").await;
        fs.write_file(Path::new("/b"), b"\xf6").await.unwrap();
        let r = run_bytes(&["-f", "latin1", "-t", "utf-8", "/a", "/b"], None, fs).await;
        assert_eq!(r.stdout, "éö");
    }

    #[tokio::test]
    async fn translit_to_ascii() {
        let r = run(
            &["-f", "UTF-8", "-t", "ascii//translit"],
            Some("café naïve"),
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "cafe naive");
    }

    #[tokio::test]
    async fn translit_multi_char_and_unknown() {
        let r = run(
            &["-f", "UTF-8", "-t", "ASCII//TRANSLIT"],
            Some("Straße 5€ Ж"),
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "Strasse 5EUR ?");
    }

    #[tokio::test]
    async fn translit_keeps_chars_the_target_holds() {
        let r = run(&["-f", "UTF-8", "-t", "latin1//TRANSLIT"], Some("é€")).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.as_bytes(), b"\xe9EUR");
    }

    // --- -o ---

    #[tokio::test]
    async fn output_file_in_place() {
        let fs = fs_with("/data.csv", b"K\xf6ln\n").await;
        let r = run_bytes(
            &["-f", "cp819", "-t", "utf-8", "-o", "/data.csv", "/data.csv"],
            None,
            fs.clone(),
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.is_empty());
        assert_eq!(
            fs.read_file(Path::new("/data.csv")).await.unwrap(),
            "Köln\n".as_bytes()
        );
    }

    #[tokio::test]
    async fn output_file_untouched_on_failure() {
        let original = b"K\xf6ln\n";
        let fs = fs_with("/data.csv", original).await;
        let r = run_bytes(
            &[
                "-f",
                "utf-8",
                "-t",
                "latin1",
                "-o",
                "/data.csv",
                "/data.csv",
            ],
            None,
            fs.clone(),
        )
        .await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("illegal input sequence at position 1"));
        assert_eq!(
            fs.read_file(Path::new("/data.csv")).await.unwrap(),
            original
        );
    }

    // --- -c / //IGNORE ---

    #[tokio::test]
    async fn omit_invalid_input_bytes() {
        let fs = Arc::new(InMemoryFs::new());
        let r = run_bytes(&["-c", "-f", "utf-8", "-t", "utf-8"], Some(b"a\xffb\n"), fs).await;
        assert_eq!(r.exit_code, 1);
        assert_eq!(r.stdout, "ab\n");
        assert!(r.stderr.is_empty());
    }

    #[tokio::test]
    async fn omit_truncated_trailing_char() {
        let fs = Arc::new(InMemoryFs::new());
        let r = run_bytes(&["-c", "-f", "utf-8", "-t", "latin1"], Some(b"a\xc3"), fs).await;
        assert_eq!(r.exit_code, 1);
        assert_eq!(r.stdout, "a");
    }

    #[tokio::test]
    async fn omit_unencodable_chars() {
        for args in [
            &["-c", "-f", "utf-8", "-t", "latin1"][..],
            &["-f", "utf-8", "-t", "latin1//IGNORE"][..],
        ] {
            let r = run(args, Some("aЖé\n")).await;
            assert_eq!(r.exit_code, 1, "{}", args.join(" "));
            assert_eq!(r.stdout.as_bytes(), b"a\xe9\n", "{}", args.join(" "));
        }
    }

    #[tokio::test]
    async fn omit_with_output_file_still_writes() {
        let fs = fs_with("/in", b"a\xffb").await;
        let r = run_bytes(
            &["-c", "-f", "utf-8", "-t", "utf-8", "-o", "/out", "/in"],
            None,
            fs.clone(),
        )
        .await;
        assert_eq!(r.exit_code, 1);
        assert_eq!(fs.read_file(Path::new("/out")).await.unwrap(), b"ab");
    }

    #[tokio::test]
    async fn omit_broken_utf16() {
        let fs = Arc::new(InMemoryFs::new());
        // lone high surrogate, then 'a', then odd trailing byte
        let r = run_bytes(
            &["-c", "-f", "utf-16le", "-t", "utf-8"],
            Some(b"\x00\xD8a\0x"),
            fs,
        )
        .await;
        assert_eq!(r.exit_code, 1);
        assert_eq!(r.stdout, "a");
    }

    // --- negative ---

    #[tokio::test]
    async fn unencodable_char_stops_with_prefix() {
        let r = run(&["-f", "UTF-8", "-t", "ASCII"], Some("caf\u{e9}!")).await;
        assert_eq!(r.exit_code, 1);
        assert_eq!(r.stdout, "caf");
        assert_eq!(r.stderr, "iconv: illegal input sequence at position 3\n");
    }

    #[tokio::test]
    async fn invalid_utf8_input_reports_position() {
        let fs = Arc::new(InMemoryFs::new());
        let r = run_bytes(&["-f", "utf-8", "-t", "utf-16le"], Some(b"ab\xffc"), fs).await;
        assert_eq!(r.exit_code, 1);
        assert_eq!(r.stdout.as_bytes(), b"a\0b\0");
        assert!(r.stderr.contains("position 2"));
    }

    #[tokio::test]
    async fn truncated_input_is_incomplete() {
        let fs = Arc::new(InMemoryFs::new());
        let r = run_bytes(
            &["-f", "utf-8", "-t", "utf-8"],
            Some(b"a\xe2\x82"),
            fs.clone(),
        )
        .await;
        assert_eq!(r.exit_code, 1);
        assert_eq!(r.stdout, "a");
        assert!(r.stderr.contains("incomplete character"));
        let r = run_bytes(&["-f", "utf-16le", "-t", "utf-8"], Some(b"a\0b"), fs).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("incomplete character"));
    }

    #[tokio::test]
    async fn invalid_ascii_input() {
        let fs = Arc::new(InMemoryFs::new());
        let r = run_bytes(&["-f", "ascii", "-t", "utf-8"], Some(b"a\x80"), fs).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("position 1"));
    }

    #[tokio::test]
    async fn missing_from_encoding() {
        let r = run(&["-t", "ASCII"], Some("hi")).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("missing source encoding"));
    }

    #[tokio::test]
    async fn missing_to_encoding() {
        let r = run(&["-f", "ASCII"], Some("hi")).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("missing target encoding"));
    }

    #[tokio::test]
    async fn missing_option_argument() {
        for flag in ["-f", "-t", "-o", "--output"] {
            let r = run(&["-f", "utf-8", "-t", "ascii", flag], Some("hi")).await;
            assert_eq!(r.exit_code, 1, "{flag}");
            assert!(r.stderr.contains("requires an argument"), "{flag}");
        }
    }

    #[tokio::test]
    async fn unsupported_encoding() {
        let r = run(&["-f", "EBCDIC", "-t", "UTF-8"], Some("hi")).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("unsupported encoding"));
    }

    #[tokio::test]
    async fn unsupported_target_suffix() {
        for target in ["ASCII//BAD", "UTF-8//TRANSLIT//NOPE"] {
            let r = run(&["-f", "UTF-8", "-t", target], Some("hi")).await;
            assert_eq!(r.exit_code, 1, "target should be rejected: {target}");
            assert!(r.stderr.contains("unsupported encoding"));
        }
    }

    #[tokio::test]
    async fn unknown_option() {
        let r = run(&["-f", "utf-8", "-t", "ascii", "-x"], Some("hi")).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("unknown option '-x'"));
    }

    #[tokio::test]
    async fn no_input() {
        let r = run(&["-f", "UTF-8", "-t", "ASCII"], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("no input"));
    }

    #[tokio::test]
    async fn file_not_found() {
        let r = run(&["-f", "UTF-8", "-t", "ASCII", "/nope.txt"], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("/nope.txt"));
    }

    #[tokio::test]
    async fn arbitrary_bytes_never_panic() {
        // Every single byte and byte pair, through every source charset.
        let fs = Arc::new(InMemoryFs::new());
        let mut inputs: Vec<Vec<u8>> = (0u8..=255).map(|b| vec![b]).collect();
        inputs.push((0u8..=255).collect());
        inputs.push(vec![0xF4, 0x90, 0x80, 0x80]);
        inputs.push(vec![0xED, 0xA0, 0x80]);
        for from in ["utf-8", "ascii", "latin1", "utf-16", "utf-16le", "utf-16be"] {
            for input in &inputs {
                for flags in [&["-c"][..], &[][..]] {
                    let mut args = flags.to_vec();
                    args.extend(["-f", from, "-t", "utf-16"]);
                    let _ = run_bytes(&args, Some(input), fs.clone()).await;
                }
            }
        }
    }
}
