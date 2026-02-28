//! File operation builtins - mkdir, rm, cp, mv, touch, chmod

use async_trait::async_trait;
use std::path::Path;

use super::{resolve_path, Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The mkdir builtin - create directories.
///
/// Usage: mkdir [-p] DIRECTORY...
///
/// Options:
///   -p   Create parent directories as needed, no error if existing
pub struct Mkdir;

#[async_trait]
impl Builtin for Mkdir {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err("mkdir: missing operand\n".to_string(), 1));
        }

        let recursive = ctx.args.iter().any(|a| a == "-p");
        let dirs: Vec<_> = ctx.args.iter().filter(|a| !a.starts_with('-')).collect();

        if dirs.is_empty() {
            return Ok(ExecResult::err("mkdir: missing operand\n".to_string(), 1));
        }

        for dir in dirs {
            let path = resolve_path(ctx.cwd, dir);

            // Check if already exists
            if ctx.fs.exists(&path).await.unwrap_or(false) {
                // Check if it's a directory or something else (file/symlink)
                if let Ok(meta) = ctx.fs.stat(&path).await {
                    if meta.file_type.is_dir() {
                        if !recursive {
                            return Ok(ExecResult::err(
                                format!("mkdir: cannot create directory '{}': File exists\n", dir),
                                1,
                            ));
                        }
                        // With -p, existing directory is not an error
                        continue;
                    }
                }
                // File or symlink exists - always an error
                return Ok(ExecResult::err(
                    format!("mkdir: cannot create directory '{}': File exists\n", dir),
                    1,
                ));
            }

            if let Err(e) = ctx.fs.mkdir(&path, recursive).await {
                return Ok(ExecResult::err(
                    format!("mkdir: cannot create directory '{}': {}\n", dir, e),
                    1,
                ));
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The rm builtin - remove files or directories.
///
/// Usage: rm [-rf] FILE...
///
/// Options:
///   -r, -R   Remove directories and their contents recursively
///   -f       Ignore nonexistent files, never prompt
pub struct Rm;

#[async_trait]
impl Builtin for Rm {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err("rm: missing operand\n".to_string(), 1));
        }

        let recursive = ctx.args.iter().any(|a| {
            a == "-r"
                || a == "-R"
                || a == "-rf"
                || a == "-fr"
                || a.contains('r') && a.starts_with('-')
        });
        let force = ctx.args.iter().any(|a| {
            a == "-f" || a == "-rf" || a == "-fr" || a.contains('f') && a.starts_with('-')
        });

        let files: Vec<_> = ctx.args.iter().filter(|a| !a.starts_with('-')).collect();

        if files.is_empty() {
            return Ok(ExecResult::err("rm: missing operand\n".to_string(), 1));
        }

        for file in files {
            let path = resolve_path(ctx.cwd, file);

            // Check if exists
            let exists = ctx.fs.exists(&path).await.unwrap_or(false);
            if !exists {
                if !force {
                    return Ok(ExecResult::err(
                        format!("rm: cannot remove '{}': No such file or directory\n", file),
                        1,
                    ));
                }
                continue;
            }

            // Check if it's a directory
            let metadata = ctx.fs.stat(&path).await;
            if let Ok(meta) = metadata {
                if meta.file_type.is_dir() && !recursive {
                    return Ok(ExecResult::err(
                        format!("rm: cannot remove '{}': Is a directory\n", file),
                        1,
                    ));
                }
            }

            if let Err(e) = ctx.fs.remove(&path, recursive).await {
                if !force {
                    return Ok(ExecResult::err(
                        format!("rm: cannot remove '{}': {}\n", file, e),
                        1,
                    ));
                }
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The cp builtin - copy files and directories.
///
/// Usage: cp [-r] SOURCE... DEST
///
/// Options:
///   -r, -R   Copy directories recursively
pub struct Cp;

#[async_trait]
impl Builtin for Cp {
    // files.last().unwrap() is safe: guarded by files.len() < 2 check above
    #[allow(clippy::unwrap_used)]
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.len() < 2 {
            return Ok(ExecResult::err("cp: missing file operand\n".to_string(), 1));
        }

        let _recursive = ctx.args.iter().any(|a| a == "-r" || a == "-R");
        let files: Vec<_> = ctx.args.iter().filter(|a| !a.starts_with('-')).collect();

        if files.len() < 2 {
            return Ok(ExecResult::err(
                "cp: missing destination file operand\n".to_string(),
                1,
            ));
        }

        let dest = files.last().unwrap();
        let sources = &files[..files.len() - 1];
        let dest_path = resolve_path(ctx.cwd, dest);

        // Check if destination is a directory
        let dest_is_dir = if let Ok(meta) = ctx.fs.stat(&dest_path).await {
            meta.file_type.is_dir()
        } else {
            false
        };

        if sources.len() > 1 && !dest_is_dir {
            return Ok(ExecResult::err(
                format!("cp: target '{}' is not a directory\n", dest),
                1,
            ));
        }

        for source in sources {
            let src_path = resolve_path(ctx.cwd, source);

            let final_dest = if dest_is_dir {
                // Copy into directory
                let filename = Path::new(source)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| source.to_string());
                dest_path.join(&filename)
            } else {
                dest_path.clone()
            };

            if let Err(e) = ctx.fs.copy(&src_path, &final_dest).await {
                return Ok(ExecResult::err(
                    format!("cp: cannot copy '{}': {}\n", source, e),
                    1,
                ));
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The mv builtin - move (rename) files.
///
/// Usage: mv SOURCE... DEST
pub struct Mv;

#[async_trait]
impl Builtin for Mv {
    // files.last().unwrap() is safe: guarded by files.len() < 2 check above
    #[allow(clippy::unwrap_used)]
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.len() < 2 {
            return Ok(ExecResult::err("mv: missing file operand\n".to_string(), 1));
        }

        let files: Vec<_> = ctx.args.iter().filter(|a| !a.starts_with('-')).collect();

        if files.len() < 2 {
            return Ok(ExecResult::err(
                "mv: missing destination file operand\n".to_string(),
                1,
            ));
        }

        let dest = files.last().unwrap();
        let sources = &files[..files.len() - 1];
        let dest_path = resolve_path(ctx.cwd, dest);

        // Check if destination is a directory
        let dest_is_dir = if let Ok(meta) = ctx.fs.stat(&dest_path).await {
            meta.file_type.is_dir()
        } else {
            false
        };

        if sources.len() > 1 && !dest_is_dir {
            return Ok(ExecResult::err(
                format!("mv: target '{}' is not a directory\n", dest),
                1,
            ));
        }

        for source in sources {
            let src_path = resolve_path(ctx.cwd, source);

            let final_dest = if dest_is_dir {
                // Move into directory
                let filename = Path::new(source)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| source.to_string());
                dest_path.join(&filename)
            } else {
                dest_path.clone()
            };

            if let Err(e) = ctx.fs.rename(&src_path, &final_dest).await {
                return Ok(ExecResult::err(
                    format!("mv: cannot move '{}': {}\n", source, e),
                    1,
                ));
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The touch builtin - change file timestamps or create empty files.
///
/// Usage: touch FILE...
pub struct Touch;

#[async_trait]
impl Builtin for Touch {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "touch: missing file operand\n".to_string(),
                1,
            ));
        }

        for file in ctx.args.iter().filter(|a| !a.starts_with('-')) {
            let path = resolve_path(ctx.cwd, file);

            // Check if file exists
            if !ctx.fs.exists(&path).await.unwrap_or(false) {
                // Create empty file
                if let Err(e) = ctx.fs.write_file(&path, &[]).await {
                    return Ok(ExecResult::err(
                        format!("touch: cannot touch '{}': {}\n", file, e),
                        1,
                    ));
                }
            }
            // For existing files, we would update mtime but VFS doesn't track it in a modifiable way
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The chmod builtin - change file mode bits.
///
/// Usage: chmod MODE FILE...
///
/// MODE can be octal (e.g., 755) or symbolic (e.g., u+x, a+r, go-w)
pub struct Chmod;

/// Parse a symbolic mode string and apply it to an existing mode.
/// Handles: [ugoa]*[+-=][rwxXst]+ (comma-separated clauses).
/// Examples: +x, u+x, a+r, go-w, u=rwx, ug+rw
fn apply_symbolic_mode(mode_str: &str, current_mode: u32) -> Option<u32> {
    let mut mode = current_mode;

    for clause in mode_str.split(',') {
        let clause = clause.trim();
        if clause.is_empty() {
            return None;
        }

        let mut chars = clause.chars().peekable();

        // Parse who: u, g, o, a (default = a if none specified)
        let mut who_u = false;
        let mut who_g = false;
        let mut who_o = false;
        let mut has_who = false;
        while let Some(&c) = chars.peek() {
            match c {
                'u' => {
                    who_u = true;
                    has_who = true;
                    chars.next();
                }
                'g' => {
                    who_g = true;
                    has_who = true;
                    chars.next();
                }
                'o' => {
                    who_o = true;
                    has_who = true;
                    chars.next();
                }
                'a' => {
                    who_u = true;
                    who_g = true;
                    who_o = true;
                    has_who = true;
                    chars.next();
                }
                _ => break,
            }
        }
        // No who specified means all (a)
        if !has_who {
            who_u = true;
            who_g = true;
            who_o = true;
        }

        // Parse operator: +, -, =
        let op = chars.next()?;
        if op != '+' && op != '-' && op != '=' {
            return None;
        }

        // Parse permissions: r, w, x, X, s, t
        let mut perm_bits: u32 = 0;
        for c in chars {
            match c {
                'r' => perm_bits |= 0o4,
                'w' => perm_bits |= 0o2,
                'x' => perm_bits |= 0o1,
                'X' => {
                    // +X: set execute only if it's a directory or already has execute
                    if current_mode & 0o111 != 0 {
                        perm_bits |= 0o1;
                    }
                }
                's' | 't' => {} // setuid/setgid/sticky: accept but ignore for VFS
                _ => return None,
            }
        }

        // Build mask for affected bits
        let mut mask: u32 = 0;
        let mut bits: u32 = 0;
        if who_u {
            mask |= 0o700;
            bits |= perm_bits << 6;
        }
        if who_g {
            mask |= 0o070;
            bits |= perm_bits << 3;
        }
        if who_o {
            mask |= 0o007;
            bits |= perm_bits;
        }

        match op {
            '+' => mode |= bits,
            '-' => mode &= !bits,
            '=' => mode = (mode & !mask) | bits,
            _ => unreachable!(),
        }
    }

    Some(mode)
}

#[async_trait]
impl Builtin for Chmod {
    // from_str_radix().unwrap() is safe: is_ok() check on same value above
    #[allow(clippy::unwrap_used)]
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.len() < 2 {
            return Ok(ExecResult::err("chmod: missing operand\n".to_string(), 1));
        }

        let mode_str = &ctx.args[0];
        let files = &ctx.args[1..];

        // Try octal first, then symbolic
        let is_octal = u32::from_str_radix(mode_str, 8).is_ok();

        for file in files.iter().filter(|a| !a.starts_with('-')) {
            let path = resolve_path(ctx.cwd, file);

            if !ctx.fs.exists(&path).await.unwrap_or(false) {
                return Ok(ExecResult::err(
                    format!(
                        "chmod: cannot access '{}': No such file or directory\n",
                        file
                    ),
                    1,
                ));
            }

            let mode = if is_octal {
                u32::from_str_radix(mode_str, 8).unwrap()
            } else {
                // Symbolic mode - need current permissions
                let current_mode = match ctx.fs.stat(&path).await {
                    Ok(meta) => meta.mode,
                    Err(_) => 0o644, // fallback default
                };
                match apply_symbolic_mode(mode_str, current_mode) {
                    Some(m) => m,
                    None => {
                        return Ok(ExecResult::err(
                            format!("chmod: invalid mode: '{}'\n", mode_str),
                            1,
                        ));
                    }
                }
            };

            if let Err(e) = ctx.fs.chmod(&path, mode).await {
                return Ok(ExecResult::err(
                    format!("chmod: changing permissions of '{}': {}\n", file, e),
                    1,
                ));
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The ln builtin - create links.
///
/// Usage: ln [-s] [-f] TARGET LINK_NAME
///        ln [-s] [-f] TARGET... DIRECTORY
///
/// Options:
///   -s   Create symbolic link (default in Bashkit; hard links not supported in VFS)
///   -f   Force: remove existing destination files
///
/// Note: In Bashkit's virtual filesystem, all links are symbolic.
/// Hard links are not supported; `-s` is implied.
pub struct Ln;

#[async_trait]
impl Builtin for Ln {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut force = false;
        let mut files: Vec<&str> = Vec::new();

        for arg in ctx.args.iter() {
            if arg.starts_with('-') && arg.len() > 1 {
                for c in arg[1..].chars() {
                    match c {
                        's' => {} // symbolic — always symbolic in VFS
                        'f' => force = true,
                        _ => {
                            return Ok(ExecResult::err(
                                format!("ln: invalid option -- '{}'\n", c),
                                1,
                            ));
                        }
                    }
                }
            } else {
                files.push(arg);
            }
        }

        if files.len() < 2 {
            return Ok(ExecResult::err("ln: missing file operand\n".to_string(), 1));
        }

        let target = files[0];
        let link_name = files[1];
        let link_path = resolve_path(ctx.cwd, link_name);

        // If link already exists
        if ctx.fs.exists(&link_path).await.unwrap_or(false) {
            if force {
                // Remove existing
                let _ = ctx.fs.remove(&link_path, false).await;
            } else {
                return Ok(ExecResult::err(
                    format!(
                        "ln: failed to create symbolic link '{}': File exists\n",
                        link_name
                    ),
                    1,
                ));
            }
        }

        let target_path = Path::new(target);
        if let Err(e) = ctx.fs.symlink(target_path, &link_path).await {
            return Ok(ExecResult::err(
                format!(
                    "ln: failed to create symbolic link '{}': {}\n",
                    link_name, e
                ),
                1,
            ));
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The chown builtin - change file ownership (no-op in VFS).
///
/// Usage: chown [-R] OWNER[:GROUP] FILE...
///
/// In the virtual filesystem there are no real UIDs/GIDs, so chown is a no-op
/// that simply validates arguments and succeeds silently.
pub struct Chown;

#[async_trait]
impl Builtin for Chown {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut recursive = false;
        let mut positional: Vec<&str> = Vec::new();

        for arg in ctx.args {
            match arg.as_str() {
                "-R" | "--recursive" => recursive = true,
                _ if arg.starts_with('-') => {} // ignore other flags
                _ => positional.push(arg),
            }
        }
        let _ = recursive; // accepted but irrelevant in VFS

        if positional.len() < 2 {
            return Ok(ExecResult::err("chown: missing operand\n".to_string(), 1));
        }

        // Validate that target files exist
        let _owner = positional[0]; // accepted but not applied
        for file in &positional[1..] {
            let path = resolve_path(ctx.cwd, file);
            if !ctx.fs.exists(&path).await.unwrap_or(false) {
                return Ok(ExecResult::err(
                    format!(
                        "chown: cannot access '{}': No such file or directory\n",
                        file
                    ),
                    1,
                ));
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The kill builtin - send signal to process (no-op in VFS).
///
/// Usage: kill [-s SIGNAL] [-SIGNAL] PID...
///
/// Since there are no real processes in the virtual environment, kill is a no-op
/// that accepts the command syntax for compatibility.
pub struct Kill;

#[async_trait]
impl Builtin for Kill {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut pids: Vec<&str> = Vec::new();

        for arg in ctx.args {
            if arg == "-l" || arg == "-L" {
                // List signal names
                return Ok(ExecResult::ok(
                    "HUP INT QUIT ILL TRAP ABRT BUS FPE KILL USR1 SEGV USR2 PIPE ALRM TERM\n"
                        .to_string(),
                ));
            }
            if arg.starts_with('-') {
                continue; // skip signal spec
            }
            pids.push(arg);
        }

        if pids.is_empty() {
            return Ok(ExecResult::err(
                "kill: usage: kill [-s sigspec | -n signum | -sigspec] pid | jobspec ...\n"
                    .to_string(),
                2,
            ));
        }

        // In VFS, no real processes exist — just succeed silently
        Ok(ExecResult::ok(String::new()))
    }
}

/// The mktemp builtin - create temporary files or directories.
///
/// Usage: mktemp [-d] [-p DIR] [-t] [TEMPLATE]
///
/// Options:
///   -d       Create a directory instead of a file
///   -p DIR   Use DIR as prefix (default: /tmp)
///   -t       Interpret TEMPLATE relative to a temp directory
pub struct Mktemp;

#[async_trait]
impl Builtin for Mktemp {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut create_dir = false;
        let mut prefix_dir = "/tmp".to_string();
        let mut template: Option<String> = None;
        let mut use_tmpdir = false;

        let mut i = 0;
        while i < ctx.args.len() {
            match ctx.args[i].as_str() {
                "-d" => create_dir = true,
                "-p" => {
                    i += 1;
                    if i < ctx.args.len() {
                        prefix_dir = ctx.args[i].clone();
                    }
                }
                "-t" => use_tmpdir = true,
                arg if !arg.starts_with('-') => {
                    template = Some(arg.to_string());
                }
                _ => {} // ignore unknown flags
            }
            i += 1;
        }

        // Generate random suffix
        use std::collections::hash_map::RandomState;
        use std::hash::{BuildHasher, Hasher};
        let random = RandomState::new().build_hasher().finish();
        let suffix = format!("{:010x}", random % 0xFF_FFFF_FFFF);

        // Build path
        let name = if let Some(tmpl) = &template {
            if tmpl.contains("XXXXXX") {
                tmpl.replacen("XXXXXX", &suffix[..6], 1)
            } else {
                format!("{}.{}", tmpl, &suffix[..6])
            }
        } else {
            format!("tmp.{}", &suffix[..10])
        };

        let path = if use_tmpdir || template.is_none() || !name.contains('/') {
            format!("{}/{}", prefix_dir, name)
        } else {
            let p = resolve_path(ctx.cwd, &name);
            p.to_string_lossy().to_string()
        };

        let full_path = std::path::PathBuf::from(&path);

        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            if !ctx.fs.exists(parent).await.unwrap_or(false) {
                let _ = ctx.fs.mkdir(parent, true).await;
            }
        }

        if create_dir {
            if let Err(e) = ctx.fs.mkdir(&full_path, true).await {
                return Ok(ExecResult::err(
                    format!("mktemp: failed to create directory '{}': {}\n", path, e),
                    1,
                ));
            }
        } else if let Err(e) = ctx.fs.write_file(&full_path, &[]).await {
            return Ok(ExecResult::err(
                format!("mktemp: failed to create file '{}': {}\n", path, e),
                1,
            ));
        }

        Ok(ExecResult::ok(format!("{}\n", path)))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    async fn create_test_ctx() -> (Arc<InMemoryFs>, PathBuf, HashMap<String, String>) {
        let fs = Arc::new(InMemoryFs::new());
        let cwd = PathBuf::from("/home/user");
        let variables = HashMap::new();

        // Create the cwd
        fs.mkdir(&cwd, true).await.unwrap();

        (fs, cwd, variables)
    }

    #[tokio::test]
    async fn test_mkdir_simple() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["testdir".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = Mkdir.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(&cwd.join("testdir")).await.unwrap());
    }

    #[tokio::test]
    async fn test_mkdir_recursive() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-p".to_string(), "a/b/c".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = Mkdir.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(&cwd.join("a/b/c")).await.unwrap());
    }

    #[tokio::test]
    async fn test_touch_create() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["newfile.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = Touch.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(&cwd.join("newfile.txt")).await.unwrap());
    }

    #[tokio::test]
    async fn test_rm_file() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create a file first
        fs.write_file(&cwd.join("testfile.txt"), b"content")
            .await
            .unwrap();

        let args = vec!["testfile.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = Rm.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!fs.exists(&cwd.join("testfile.txt")).await.unwrap());
    }

    #[tokio::test]
    async fn test_rm_force_nonexistent() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-f".to_string(), "nonexistent".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = Rm.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0); // No error with -f
    }

    #[tokio::test]
    async fn test_cp_file() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create source file
        fs.write_file(&cwd.join("source.txt"), b"content")
            .await
            .unwrap();

        let args = vec!["source.txt".to_string(), "dest.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = Cp.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(&cwd.join("dest.txt")).await.unwrap());

        let content = fs.read_file(&cwd.join("dest.txt")).await.unwrap();
        assert_eq!(content, b"content");
    }

    #[tokio::test]
    async fn test_mv_file() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create source file
        fs.write_file(&cwd.join("source.txt"), b"content")
            .await
            .unwrap();

        let args = vec!["source.txt".to_string(), "dest.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = Mv.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!fs.exists(&cwd.join("source.txt")).await.unwrap());
        assert!(fs.exists(&cwd.join("dest.txt")).await.unwrap());
    }

    #[tokio::test]
    async fn test_chmod_octal() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create a file
        fs.write_file(&cwd.join("script.sh"), b"#!/bin/bash")
            .await
            .unwrap();

        let args = vec!["755".to_string(), "script.sh".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = Chmod.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);

        let meta = fs.stat(&cwd.join("script.sh")).await.unwrap();
        assert_eq!(meta.mode, 0o755);
    }
}
