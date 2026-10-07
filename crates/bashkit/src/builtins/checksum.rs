//! Checksum builtins - md5sum, sha1sum, sha2 family, b2sum, cksum
//!
//! Decision: one generic driver (`checksum_execute`) for every `*sum` tool,
//! including GNU `-c/--check` verification, since agents use
//! `sha256sum -c SUMS` to verify downloads. `cksum` is POSIX CRC-32 with the
//! byte length folded in, computed by a small table-driven implementation
//! rather than a crate.

use async_trait::async_trait;
use blake2::Blake2b512;
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha224, Sha256, Sha384, Sha512};

use super::{Builtin, Context};
use crate::error::Result;
use crate::fs::vfs_join;
use crate::interpreter::ExecResult;

macro_rules! digest_builtin {
    ($ty:ident, $name:literal, $label:literal, $digest:ty) => {
        #[doc = concat!("`", $name, "` builtin - compute ", $label, " message digests")]
        pub struct $ty;

        #[async_trait]
        impl Builtin for $ty {
            async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
                if let Some(r) = super::check_help_version(
                    ctx.args,
                    concat!(
                        "Usage: ",
                        $name,
                        " [OPTION]... [FILE]...\n",
                        "Print or check ",
                        $label,
                        " checksums.\n\n",
                        "  -c, --check\tread checksums from the FILEs and check them\n",
                        "  --quiet\tdon't print OK for each successfully verified file\n",
                        "  --status\tdon't output anything, status code shows success\n",
                        "  -b, -t\taccepted for compatibility (no effect)\n",
                        "  --help\tdisplay this help and exit\n",
                        "  --version\toutput version information and exit\n"
                    ),
                    Some(concat!($name, " (bashkit) 0.1")),
                ) {
                    return Ok(r);
                }
                checksum_execute::<$digest>(&ctx, $name).await
            }
        }
    };
}

digest_builtin!(Md5sum, "md5sum", "MD5", Md5);
digest_builtin!(Sha1sum, "sha1sum", "SHA1", Sha1);
digest_builtin!(Sha224sum, "sha224sum", "SHA224", Sha224);
digest_builtin!(Sha256sum, "sha256sum", "SHA256", Sha256);
digest_builtin!(Sha384sum, "sha384sum", "SHA384", Sha384);
digest_builtin!(Sha512sum, "sha512sum", "SHA512", Sha512);
digest_builtin!(B2sum, "b2sum", "BLAKE2b-512", Blake2b512);

#[derive(Default)]
struct CheckOpts {
    check: bool,
    quiet: bool,
    status: bool,
}

async fn read_operand(ctx: &Context<'_>, file: &str) -> std::result::Result<Vec<u8>, String> {
    if file == "-" {
        return Ok(ctx.stdin.map(|s| s.as_bytes().to_vec()).unwrap_or_default());
    }
    let path = if file.starts_with('/') {
        std::path::PathBuf::from(file)
    } else {
        vfs_join(ctx.cwd, file)
    };
    ctx.fs.read_file(&path).await.map_err(|e| e.to_string())
}

async fn checksum_execute<D: Digest>(ctx: &Context<'_>, cmd: &str) -> Result<ExecResult> {
    let mut files: Vec<&str> = Vec::new();
    let mut end_of_options = false;
    let mut opts = CheckOpts::default();

    for arg in ctx.args {
        if end_of_options || arg == "-" || !arg.starts_with('-') {
            files.push(arg);
            continue;
        }
        match arg.as_str() {
            "--" => end_of_options = true,
            "--check" => opts.check = true,
            "--quiet" => opts.quiet = true,
            "--status" => opts.status = true,
            "--binary" | "--text" | "--warn" | "--strict" | "--ignore-missing" => {}
            long if long.starts_with("--") => {
                return Ok(ExecResult::err(
                    format!("{}: unrecognized option '{}'\n", cmd, long),
                    1,
                ));
            }
            short => {
                for c in short[1..].chars() {
                    match c {
                        'c' => opts.check = true,
                        'b' | 't' | 'w' => {}
                        _ => {
                            return Ok(ExecResult::err(
                                format!("{}: invalid option -- '{}'\n", cmd, c),
                                1,
                            ));
                        }
                    }
                }
            }
        }
    }
    if files.is_empty() {
        files.push("-");
    }

    if opts.check {
        return check_sums::<D>(ctx, cmd, &files, &opts).await;
    }

    let mut output = String::new();
    for file in &files {
        match read_operand(ctx, file).await {
            Ok(content) => {
                output.push_str(&hex_digest::<D>(&content));
                output.push_str("  ");
                output.push_str(file);
                output.push('\n');
            }
            Err(e) => {
                return Ok(ExecResult::err(format!("{}: {}: {}\n", cmd, file, e), 1));
            }
        }
    }
    Ok(ExecResult::ok(output))
}

/// `-c`: each line is `HASH  NAME` or `HASH *NAME`.
async fn check_sums<D: Digest>(
    ctx: &Context<'_>,
    cmd: &str,
    files: &[&str],
    opts: &CheckOpts,
) -> Result<ExecResult> {
    let hex_len = <D as Digest>::output_size() * 2;
    let mut out = String::new();
    let mut err = String::new();
    let (mut failed, mut unreadable, mut malformed, mut verified) =
        (0usize, 0usize, 0usize, 0usize);

    for list in files {
        let content = match read_operand(ctx, list).await {
            Ok(c) => c,
            Err(e) => {
                err.push_str(&format!("{}: {}: {}\n", cmd, list, e));
                unreadable += 1;
                continue;
            }
        };
        let text = String::from_utf8_lossy(&content);
        for line in text.lines() {
            let Some((hash, name)) = parse_check_line(line, hex_len) else {
                if !line.trim().is_empty() && !line.starts_with('#') {
                    malformed += 1;
                }
                continue;
            };
            match read_operand(ctx, name).await {
                Ok(data) => {
                    verified += 1;
                    if hex_digest::<D>(&data).eq_ignore_ascii_case(hash) {
                        if !opts.quiet {
                            out.push_str(&format!("{name}: OK\n"));
                        }
                    } else {
                        failed += 1;
                        out.push_str(&format!("{name}: FAILED\n"));
                    }
                }
                Err(e) => {
                    unreadable += 1;
                    err.push_str(&format!("{cmd}: {name}: {e}\n"));
                    out.push_str(&format!("{name}: FAILED open or read\n"));
                }
            }
        }
    }

    if verified == 0 && unreadable == 0 {
        err.push_str(&format!(
            "{cmd}: {}: no properly formatted checksum lines found\n",
            files.join(" ")
        ));
        return Ok(finish(out, err, 1, opts));
    }
    if malformed > 0 {
        let s = if malformed == 1 {
            "line is"
        } else {
            "lines are"
        };
        err.push_str(&format!(
            "{cmd}: WARNING: {malformed} {s} improperly formatted\n"
        ));
    }
    if unreadable > 0 {
        let s = if unreadable == 1 { "file" } else { "files" };
        err.push_str(&format!(
            "{cmd}: WARNING: {unreadable} listed {s} could not be read\n"
        ));
    }
    if failed > 0 {
        let s = if failed == 1 { "checksum" } else { "checksums" };
        err.push_str(&format!(
            "{cmd}: WARNING: {failed} computed {s} did NOT match\n"
        ));
    }
    let code = i32::from(failed > 0 || unreadable > 0);
    Ok(finish(out, err, code, opts))
}

fn finish(out: String, err: String, code: i32, opts: &CheckOpts) -> ExecResult {
    if opts.status {
        return ExecResult {
            exit_code: code,
            ..Default::default()
        };
    }
    ExecResult {
        stdout: out.into(),
        stderr: err.into(),
        exit_code: code,
        ..Default::default()
    }
}

fn parse_check_line(line: &str, hex_len: usize) -> Option<(&str, &str)> {
    let hash = line.get(..hex_len)?;
    if !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let rest = &line[hex_len..];
    let name = rest
        .strip_prefix("  ")
        .or_else(|| rest.strip_prefix(" *"))?;
    (!name.is_empty()).then_some((hash, name))
}

fn hex_digest<D: Digest>(data: &[u8]) -> String {
    let result = D::digest(data);
    result.iter().map(|b| format!("{:02x}", b)).collect()
}

/// `cksum` builtin - POSIX CRC-32 checksum and byte count.
pub struct Cksum;

/// POSIX `cksum` CRC: polynomial 0x04C11DB7, MSB-first, length appended.
fn posix_cksum(data: &[u8]) -> u32 {
    fn update(mut crc: u32, byte: u8) -> u32 {
        crc ^= u32::from(byte) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04C1_1DB7
            } else {
                crc << 1
            };
        }
        crc
    }
    let mut crc = data.iter().fold(0u32, |c, &b| update(c, b));
    let mut len = data.len() as u64;
    while len > 0 {
        crc = update(crc, (len & 0xff) as u8);
        len >>= 8;
    }
    !crc
}

#[async_trait]
impl Builtin for Cksum {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: cksum [FILE]...\nPrint CRC checksum and byte counts of each FILE.\n\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("cksum (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let mut files: Vec<&str> = Vec::new();
        for arg in ctx.args {
            if arg.starts_with('-') && arg != "-" && arg != "--" {
                return Ok(ExecResult::err(
                    format!("cksum: unrecognized option '{arg}'\n"),
                    1,
                ));
            }
            if arg != "--" {
                files.push(arg);
            }
        }
        let stdin_only = files.is_empty();
        if stdin_only {
            files.push("-");
        }
        let mut out = String::new();
        for file in files {
            match read_operand(&ctx, file).await {
                Ok(data) => {
                    out.push_str(&format!("{} {}", posix_cksum(&data), data.len()));
                    if !stdin_only {
                        out.push(' ');
                        out.push_str(file);
                    }
                    out.push('\n');
                }
                Err(e) => return Ok(ExecResult::err(format!("cksum: {file}: {e}\n"), 1)),
            }
        }
        Ok(ExecResult::ok(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_checksum<B: Builtin>(
        builtin: &B,
        args: &[&str],
        stdin: Option<&str>,
    ) -> ExecResult {
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

        builtin.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_md5sum_stdin() {
        let result = run_checksum(&Md5sum, &[], Some("hello\n")).await;
        assert_eq!(result.exit_code, 0);
        // md5("hello\n") = b1946ac92492d2347c6235b4d2611184
        assert!(
            result
                .stdout
                .starts_with("b1946ac92492d2347c6235b4d2611184")
        );
        assert!(result.stdout.contains("  -"));
    }

    #[tokio::test]
    async fn test_sha256sum_stdin() {
        let result = run_checksum(&Sha256sum, &[], Some("hello\n")).await;
        assert_eq!(result.exit_code, 0);
        // sha256("hello\n") = 5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03
        assert!(
            result
                .stdout
                .starts_with("5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03")
        );
    }

    #[tokio::test]
    async fn test_sha1sum_stdin() {
        let result = run_checksum(&Sha1sum, &[], Some("hello\n")).await;
        assert_eq!(result.exit_code, 0);
        // sha1("hello\n") = f572d396fae9206628714fb2ce00f72e94f2258f
        assert!(
            result
                .stdout
                .starts_with("f572d396fae9206628714fb2ce00f72e94f2258f")
        );
    }

    #[tokio::test]
    async fn sha256sum_check_verifies_stdin_list() {
        let sums = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03  -\n";
        // `-` names stdin, which is also the list here; GNU behaves the same.
        let result = run_checksum(&Sha256sum, &["-c"], Some(sums)).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("-: FAILED"));
        assert!(result.stderr.contains("1 computed checksum did NOT match"));
    }

    #[tokio::test]
    async fn sha256sum_check_rejects_list_without_checksums() {
        let result = run_checksum(&Sha256sum, &["--check"], Some("junk\n")).await;
        assert_eq!(result.exit_code, 1);
        assert!(
            result
                .stderr
                .contains("no properly formatted checksum lines found")
        );
    }

    #[tokio::test]
    async fn checksum_unknown_options_are_rejected() {
        let result = run_checksum(&Sha256sum, &["-z"], Some("")).await;
        assert!(result.stderr.contains("sha256sum: invalid option -- 'z'"));
        let result = run_checksum(&Sha256sum, &["--nope"], Some("")).await;
        assert!(
            result
                .stderr
                .contains("sha256sum: unrecognized option '--nope'")
        );
    }

    #[tokio::test]
    async fn sha2_family_and_b2sum_known_vectors() {
        let r = run_checksum(&Sha512sum, &[], Some("")).await;
        assert!(
            r.stdout
                .starts_with("cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce")
        );
        let r = run_checksum(&Sha224sum, &[], Some("")).await;
        assert!(
            r.stdout
                .starts_with("d14a028c2a3a2bc9476102bb288234c415a2b01f828ea62ac5b3e42f")
        );
        let r = run_checksum(&Sha384sum, &[], Some("")).await;
        assert!(
            r.stdout
                .starts_with("38b060a751ac96384cd9327eb1b1e36a21fdb71114be0743")
        );
        let r = run_checksum(&B2sum, &[], Some("")).await;
        assert!(
            r.stdout
                .starts_with("786a02f742015903c6c6fd852552d272912f4740e15847618a86e217f71f5419")
        );
    }

    #[test]
    fn posix_cksum_matches_gnu() {
        // `printf 'hello\n' | cksum` => 3015617425 6; empty => 4294967295 0
        assert_eq!(posix_cksum(b"hello\n"), 3_015_617_425);
        assert_eq!(posix_cksum(b""), 4_294_967_295);
    }

    #[tokio::test]
    async fn test_sha256sum_dash_reads_stdin() {
        let result = run_checksum(&Sha256sum, &["-"], Some("hello\n")).await;
        assert_eq!(result.exit_code, 0);
        assert!(
            result
                .stdout
                .starts_with("5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03")
        );
        assert!(result.stdout.contains("  -"));
    }

    #[tokio::test]
    async fn test_md5sum_empty() {
        let result = run_checksum(&Md5sum, &[], Some("")).await;
        assert_eq!(result.exit_code, 0);
        // md5("") = d41d8cd98f00b204e9800998ecf8427e
        assert!(
            result
                .stdout
                .starts_with("d41d8cd98f00b204e9800998ecf8427e")
        );
    }
}
