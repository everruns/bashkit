//! Small coreutils agents expect: egrep, fgrep, link, unlink, chgrp,
//! shasum, sum.
//!
//! Decision: thin front ends over existing builtins (`grep`, `chown`, the
//! checksum driver) so behavior stays in one place. `link` makes a symbolic
//! link, like `ln` without `-s` (L-FS-001: the VFS has no inodes to share).

use async_trait::async_trait;
use sha1::Sha1;
use sha2::{Sha224, Sha256, Sha384, Sha512};

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// Re-run a builtin with different arguments.
async fn run_with_args(
    builtin: &dyn Builtin,
    ctx: Context<'_>,
    args: &[String],
) -> Result<ExecResult> {
    let new_ctx = Context {
        args,
        env: ctx.env,
        variables: ctx.variables,
        cwd: ctx.cwd,
        fs: ctx.fs,
        stdin: ctx.stdin,
        #[cfg(feature = "http_client")]
        http_client: ctx.http_client,
        #[cfg(feature = "git")]
        git_client: ctx.git_client,
        #[cfg(feature = "ssh")]
        ssh_client: ctx.ssh_client,
        shell: ctx.shell,
    };
    builtin.execute(new_ctx).await
}

/// `egrep` / `fgrep`: `grep -E` / `grep -F`. GNU grep 3.8+ also warns that
/// they are obsolescent; that warning is left out.
pub struct GrepAlias {
    flag: &'static str,
}

impl GrepAlias {
    /// `egrep` = `grep -E`
    pub fn egrep() -> Self {
        Self { flag: "-E" }
    }
    /// `fgrep` = `grep -F`
    pub fn fgrep() -> Self {
        Self { flag: "-F" }
    }
}

#[async_trait]
impl Builtin for GrepAlias {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut args = vec![self.flag.to_string()];
        args.extend(ctx.args.iter().cloned());
        run_with_args(&super::Grep, ctx, &args).await
    }
}

/// `chgrp GROUP FILE...`: ownership is not modeled, like `chown`.
pub struct Chgrp;

#[async_trait]
impl Builtin for Chgrp {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: chgrp [OPTION]... GROUP FILE...\nChange the group of each FILE to GROUP.\n\n  -R, --recursive\toperate on files and directories recursively\n",
            Some("chgrp (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let args = ctx.args.to_vec();
        let mut r = run_with_args(&super::Chown, ctx, &args).await?;
        if !r.stderr.is_empty() {
            r.stderr = r.stderr.text_lossy().replace("chown", "chgrp").into();
        }
        Ok(r)
    }
}

/// `link FILE1 FILE2` and `unlink FILE`.
pub struct Link {
    unlink: bool,
}

impl Link {
    /// `link FILE1 FILE2`
    pub fn hard() -> Self {
        Self { unlink: false }
    }
    /// `unlink FILE`
    pub fn unlink() -> Self {
        Self { unlink: true }
    }
}

#[async_trait]
impl Builtin for Link {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let (name, usage, want) = if self.unlink {
            (
                "unlink",
                "Usage: unlink FILE\nCall the unlink function to remove the specified FILE.\n",
                1,
            )
        } else {
            (
                "link",
                "Usage: link FILE1 FILE2\nCall the link function to create a link named FILE2 to an existing FILE1.\n",
                2,
            )
        };
        if let Some(r) =
            super::check_help_version(ctx.args, usage, Some(&format!("{name} (bashkit) 0.1")))
        {
            return Ok(r);
        }
        let operands: Vec<&String> = match ctx.args.first().map(String::as_str) {
            Some("--") => ctx.args[1..].iter().collect(),
            _ => ctx.args.iter().collect(),
        };
        if let Some(opt) = operands.iter().find(|a| a.starts_with('-') && a.len() > 1)
            && ctx.args.first().map(String::as_str) != Some("--")
        {
            return Ok(super::invalid_option(name, opt, 1));
        }
        if operands.len() < want {
            let msg = match operands.last() {
                Some(a) => format!("{name}: missing operand after '{a}'\n"),
                None => format!("{name}: missing operand\n"),
            };
            return Ok(ExecResult::err(
                format!("{msg}Try '{name} --help' for more information.\n"),
                1,
            ));
        }
        if operands.len() > want {
            return Ok(ExecResult::err(
                format!(
                    "{name}: extra operand '{}'\nTry '{name} --help' for more information.\n",
                    operands[want]
                ),
                1,
            ));
        }
        if self.unlink {
            let file = operands[0];
            let path = super::resolve_path(ctx.cwd, file);
            let meta = match ctx.fs.lstat(&path).await {
                Ok(m) => m,
                Err(_) => {
                    return Ok(ExecResult::err(
                        format!("unlink: cannot unlink '{file}': No such file or directory\n"),
                        1,
                    ));
                }
            };
            if meta.file_type.is_dir() {
                return Ok(ExecResult::err(
                    format!("unlink: cannot unlink '{file}': Is a directory\n"),
                    1,
                ));
            }
            if let Err(e) = ctx.fs.remove(&path, false).await {
                return Ok(ExecResult::err(
                    format!("unlink: cannot unlink '{file}': {e}\n"),
                    1,
                ));
            }
            return Ok(ExecResult::ok(String::new()));
        }
        let (from, to) = (operands[0], operands[1]);
        let from_path = super::resolve_path(ctx.cwd, from);
        let to_path = super::resolve_path(ctx.cwd, to);
        if !ctx.fs.exists(&from_path).await.unwrap_or(false) {
            return Ok(ExecResult::err(
                format!("link: cannot create link '{to}' to '{from}': No such file or directory\n"),
                1,
            ));
        }
        if ctx.fs.lstat(&to_path).await.is_ok() {
            return Ok(ExecResult::err(
                format!("link: cannot create link '{to}' to '{from}': File exists\n"),
                1,
            ));
        }
        if let Err(e) = ctx.fs.symlink(&from_path, &to_path).await {
            return Ok(ExecResult::err(
                format!("link: cannot create link '{to}' to '{from}': {e}\n"),
                1,
            ));
        }
        Ok(ExecResult::ok(String::new()))
    }
}

/// `shasum [-a ALG] ...`: the Perl front end over the SHA family.
pub struct Shasum;

#[async_trait]
impl Builtin for Shasum {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: shasum [OPTION]... [FILE]...\nPrint or check SHA checksums.\n\n  -a, --algorithm\t1 (default), 224, 256, 384, 512\n  -c, --check\tread SHA sums from the FILEs and check them\n  -b, -t\taccepted for compatibility (no effect)\n",
            Some("shasum (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let mut alg = "1".to_string();
        let mut rest: Vec<String> = Vec::new();
        let mut it = ctx.args.iter();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "-a" | "--algorithm" => match it.next() {
                    Some(v) => alg = v.clone(),
                    None => {
                        return Ok(ExecResult::err(
                            "shasum: option requires an argument -- 'a'\n".to_string(),
                            1,
                        ));
                    }
                },
                a if a.starts_with("--algorithm=") => alg = a[12..].to_string(),
                a if a.starts_with("-a") && a.len() > 2 => alg = a[2..].to_string(),
                "-U" | "--UNIVERSAL" | "-0" | "--01" => {}
                _ => rest.push(arg.clone()),
            }
        }
        let cmd = "shasum";
        match alg.as_str() {
            "1" => super::checksum::checksum_execute_args::<Sha1>(&ctx, cmd, &rest).await,
            "224" => super::checksum::checksum_execute_args::<Sha224>(&ctx, cmd, &rest).await,
            "256" => super::checksum::checksum_execute_args::<Sha256>(&ctx, cmd, &rest).await,
            "384" => super::checksum::checksum_execute_args::<Sha384>(&ctx, cmd, &rest).await,
            "512" => super::checksum::checksum_execute_args::<Sha512>(&ctx, cmd, &rest).await,
            other => Ok(ExecResult::err(
                format!("shasum: Unrecognized algorithm '{other}'\n"),
                1,
            )),
        }
    }
}

/// BSD 16-bit rotating checksum (`sum -r`, the GNU default).
fn bsd_sum(data: &[u8]) -> (u32, u64) {
    let mut checksum: u32 = 0;
    for &b in data {
        checksum = (checksum >> 1) + ((checksum & 1) << 15);
        checksum = (checksum + u32::from(b)) & 0xffff;
    }
    (checksum, (data.len() as u64).div_ceil(1024))
}

/// System V checksum (`sum -s`).
fn sysv_sum(data: &[u8]) -> (u32, u64) {
    let s: u32 = data.iter().fold(0u32, |a, &b| a.wrapping_add(u32::from(b)));
    let r = (s & 0xffff) + (s >> 16);
    ((r & 0xffff) + (r >> 16), (data.len() as u64).div_ceil(512))
}

/// `sum [-r|-s] [FILE]...`
pub struct Sum;

#[async_trait]
impl Builtin for Sum {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: sum [OPTION]... [FILE]...\nPrint checksum and block counts for each FILE.\n\n  -r\tuse BSD sum algorithm (the default), use 1K blocks\n  -s, --sysv\tuse System V sum algorithm, use 512 bytes blocks\n",
            Some("sum (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let mut sysv = false;
        let mut files: Vec<&str> = Vec::new();
        for arg in ctx.args {
            match arg.as_str() {
                "-r" => sysv = false,
                "-s" | "--sysv" => sysv = true,
                "-" => files.push("-"),
                a if a.starts_with('-') => return Ok(super::invalid_option("sum", a, 1)),
                a => files.push(a),
            }
        }
        let named = !files.is_empty();
        if files.is_empty() {
            files.push("-");
        }
        let mut out = String::new();
        let mut err = String::new();
        for file in files {
            let data = if file == "-" {
                ctx.stdin.map(|s| s.as_bytes().to_vec()).unwrap_or_default()
            } else {
                let path = super::resolve_path(ctx.cwd, file);
                match ctx.fs.read_file(&path).await {
                    Ok(d) => d,
                    Err(_) => {
                        err.push_str(&format!("sum: {file}: No such file or directory\n"));
                        continue;
                    }
                }
            };
            let line = if sysv {
                let (c, blocks) = sysv_sum(&data);
                format!("{c} {blocks}")
            } else {
                let (c, blocks) = bsd_sum(&data);
                format!("{c:05} {blocks:>5}")
            };
            out.push_str(&line);
            if named && file != "-" {
                out.push(' ');
                out.push_str(file);
            }
            out.push('\n');
        }
        let mut r = ExecResult::with_code(out, i32::from(!err.is_empty()));
        r.stderr = err.into();
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sum_matches_gnu() {
        assert_eq!(bsd_sum(b"hello\n"), (36979, 1));
        assert_eq!(sysv_sum(b"hello\n"), (542, 1));
        let big = vec![b'x'; 3000];
        assert_eq!(bsd_sum(&big), (5357, 3));
        assert_eq!(sysv_sum(&big), (32325, 6));
    }
}
