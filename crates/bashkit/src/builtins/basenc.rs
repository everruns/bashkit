//! base32 and basenc builtins - RFC 4648 encodings
//!
//! Decision: base32/base16 are a few lines each, so they are implemented here
//! rather than pulling in an encoding crate; base64 variants reuse the
//! `base64` crate the `base64` builtin already depends on.

use async_trait::async_trait;
use base64::Engine;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const B32HEX: &[u8; 32] = b"0123456789ABCDEFGHIJKLMNOPQRSTUV";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Base64,
    Base64Url,
    Base32,
    Base32Hex,
    Base16,
}

fn encode(enc: Encoding, data: &[u8]) -> String {
    match enc {
        Encoding::Base64 => base64::engine::general_purpose::STANDARD.encode(data),
        Encoding::Base64Url => base64::engine::general_purpose::URL_SAFE.encode(data),
        Encoding::Base32 => base32_encode(data, B32),
        Encoding::Base32Hex => base32_encode(data, B32HEX),
        Encoding::Base16 => data.iter().map(|b| format!("{b:02X}")).collect(),
    }
}

fn decode(enc: Encoding, text: &[u8]) -> Option<Vec<u8>> {
    match enc {
        Encoding::Base64 => base64::engine::general_purpose::STANDARD.decode(text).ok(),
        Encoding::Base64Url => base64::engine::general_purpose::URL_SAFE.decode(text).ok(),
        Encoding::Base32 => base32_decode(text, B32),
        Encoding::Base32Hex => base32_decode(text, B32HEX),
        Encoding::Base16 => {
            if !text.len().is_multiple_of(2) {
                return None;
            }
            text.chunks(2)
                .map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok())
                .collect()
        }
    }
}

fn base32_encode(data: &[u8], alphabet: &[u8; 32]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(5) * 8);
    for chunk in data.chunks(5) {
        let mut buf = [0u8; 5];
        buf[..chunk.len()].copy_from_slice(chunk);
        let bits = buf.iter().fold(0u64, |acc, &b| (acc << 8) | u64::from(b));
        let chars = (chunk.len() * 8).div_ceil(5);
        for i in 0..8 {
            if i < chars {
                let idx = (bits >> (35 - i * 5)) & 0x1f;
                out.push(alphabet[idx as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn base32_decode(text: &[u8], alphabet: &[u8; 32]) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(8) {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 8 * 5);
    for chunk in text.chunks(8) {
        let pad = chunk.iter().rev().take_while(|&&c| c == b'=').count();
        let bytes = match pad {
            0 => 5,
            1 => 4,
            3 => 3,
            4 => 2,
            6 => 1,
            _ => return None,
        };
        let mut bits = 0u64;
        for &c in &chunk[..8 - pad] {
            let v = alphabet.iter().position(|&a| a == c.to_ascii_uppercase())?;
            bits = (bits << 5) | v as u64;
        }
        bits <<= 5 * pad as u64;
        for i in 0..bytes {
            out.push((bits >> (32 - i * 8)) as u8);
        }
    }
    Some(out)
}

fn wrap_lines(encoded: &str, wrap: usize) -> String {
    let mut out = String::with_capacity(encoded.len() + encoded.len() / wrap.max(1) + 1);
    if wrap == 0 {
        out.push_str(encoded);
    } else {
        for (i, chunk) in encoded.as_bytes().chunks(wrap).enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        }
    }
    if !encoded.is_empty() {
        out.push('\n');
    }
    out
}

async fn run(ctx: Context<'_>, name: &str, fixed: Option<Encoding>) -> Result<ExecResult> {
    let mut enc = fixed;
    let mut decode_mode = false;
    let mut ignore_garbage = false;
    let mut wrap = 76usize;
    let mut file: Option<&str> = None;
    let mut args = ctx.args.iter();
    while let Some(arg) = args.next() {
        let a = arg.as_str();
        match a {
            "-d" | "--decode" => decode_mode = true,
            "-i" | "--ignore-garbage" => ignore_garbage = true,
            "-w" | "--wrap" => match args.next().and_then(|v| v.parse().ok()) {
                Some(w) => wrap = w,
                None => {
                    return Ok(ExecResult::err(format!("{name}: invalid wrap size\n"), 1));
                }
            },
            _ if a.starts_with("--wrap=") => match a[7..].parse() {
                Ok(w) => wrap = w,
                Err(_) => {
                    return Ok(ExecResult::err(format!("{name}: invalid wrap size\n"), 1));
                }
            },
            _ if a.starts_with("-w") && a.len() > 2 => match a[2..].parse() {
                Ok(w) => wrap = w,
                Err(_) => {
                    return Ok(ExecResult::err(format!("{name}: invalid wrap size\n"), 1));
                }
            },
            "--base64" if fixed.is_none() => enc = Some(Encoding::Base64),
            "--base64url" if fixed.is_none() => enc = Some(Encoding::Base64Url),
            "--base32" if fixed.is_none() => enc = Some(Encoding::Base32),
            "--base32hex" if fixed.is_none() => enc = Some(Encoding::Base32Hex),
            "--base16" if fixed.is_none() => enc = Some(Encoding::Base16),
            "-" => file = Some(a),
            _ if a.starts_with('-') => {
                return Ok(ExecResult::err(
                    format!("{name}: unrecognized option '{a}'\n"),
                    1,
                ));
            }
            _ => {
                if file.is_some() {
                    return Ok(ExecResult::err(format!("{name}: extra operand '{a}'\n"), 1));
                }
                file = Some(a);
            }
        }
    }
    let Some(enc) = enc else {
        return Ok(ExecResult::err(
            format!("{name}: missing encoding type\nTry '{name} --help' for more information.\n"),
            1,
        ));
    };

    let input = match file {
        None | Some("-") => ctx.stdin_bytes().unwrap_or_default().to_vec(),
        Some(path) => {
            let resolved = super::resolve_path(ctx.cwd, path);
            match ctx.fs.read_file(&resolved).await {
                Ok(bytes) => bytes,
                Err(_) => {
                    return Ok(ExecResult::err(
                        format!("{name}: {path}: No such file or directory\n"),
                        1,
                    ));
                }
            }
        }
    };

    if decode_mode {
        let alphabet_ok = |b: &u8| match enc {
            Encoding::Base16 => b.is_ascii_hexdigit(),
            Encoding::Base32 => B32.contains(&b.to_ascii_uppercase()) || *b == b'=',
            Encoding::Base32Hex => B32HEX.contains(&b.to_ascii_uppercase()) || *b == b'=',
            Encoding::Base64 => b.is_ascii_alphanumeric() || b"+/=".contains(b),
            Encoding::Base64Url => b.is_ascii_alphanumeric() || b"-_=".contains(b),
        };
        let cleaned: Vec<u8> = input
            .into_iter()
            .filter(|b| !b.is_ascii_whitespace() && (!ignore_garbage || alphabet_ok(b)))
            .collect();
        return Ok(match decode(enc, &cleaned) {
            Some(bytes) => ExecResult::ok_bytes(bytes),
            None => ExecResult::err(format!("{name}: invalid input\n"), 1),
        });
    }
    Ok(ExecResult::ok(wrap_lines(&encode(enc, &input), wrap)))
}

const BASE32_HELP: &str = "Usage: base32 [OPTION]... [FILE]\nBase32 encode or decode FILE, or standard input.\n\n  -d, --decode\tdecode data\n  -i, --ignore-garbage\twhen decoding, ignore non-alphabet characters\n  -w, --wrap=COLS\twrap encoded lines after COLS characters (default 76, 0 disables)\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n";

const BASENC_HELP: &str = "Usage: basenc [OPTION]... [FILE]\nbasenc encode or decode FILE, or standard input.\n\n  --base64\tsame as 'base64'\n  --base64url\tfile- and url-safe base64\n  --base32\tsame as 'base32'\n  --base32hex\textended hex alphabet base32\n  --base16\thex encoding\n  -d, --decode\tdecode data\n  -i, --ignore-garbage\twhen decoding, ignore non-alphabet characters\n  -w, --wrap=COLS\twrap encoded lines after COLS characters (default 76, 0 disables)\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n";

/// `base32` builtin - RFC 4648 base32 encode/decode.
pub struct Base32;

#[async_trait]
impl Builtin for Base32 {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) =
            super::check_help_version(ctx.args, BASE32_HELP, Some("base32 (bashkit) 0.1"))
        {
            return Ok(r);
        }
        run(ctx, "base32", Some(Encoding::Base32)).await
    }
}

/// `basenc` builtin - choose an RFC 4648 encoding by flag.
pub struct Basenc;

#[async_trait]
impl Builtin for Basenc {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) =
            super::check_help_version(ctx.args, BASENC_HELP, Some("basenc (bashkit) 0.1"))
        {
            return Ok(r);
        }
        run(ctx, "basenc", None).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_rfc4648_vectors() {
        let cases = [
            ("", ""),
            ("f", "MY======"),
            ("fo", "MZXQ===="),
            ("foo", "MZXW6==="),
            ("foob", "MZXW6YQ="),
            ("fooba", "MZXW6YTB"),
            ("foobar", "MZXW6YTBOI======"),
        ];
        for (plain, enc) in cases {
            assert_eq!(base32_encode(plain.as_bytes(), B32), enc);
            assert_eq!(
                base32_decode(enc.as_bytes(), B32).unwrap(),
                plain.as_bytes()
            );
        }
        assert_eq!(base32_encode(b"foobar", B32HEX), "CPNMUOJ1E8======");
    }

    #[test]
    fn base32_rejects_bad_input() {
        assert!(base32_decode(b"MY=====", B32).is_none());
        assert!(base32_decode(b"M1======", B32).is_none());
        assert!(base32_decode(b"MY==M===", B32).is_none());
    }

    #[test]
    fn base16_round_trip() {
        assert_eq!(encode(Encoding::Base16, b"\x00\xffA"), "00FF41");
        assert_eq!(decode(Encoding::Base16, b"00ff41").unwrap(), b"\x00\xffA");
        assert!(decode(Encoding::Base16, b"0").is_none());
    }
}
