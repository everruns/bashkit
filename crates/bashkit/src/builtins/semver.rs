//! Semver builtin - semantic versioning operations
//!
//! Non-standard builtin for parsing, comparing, and manipulating semver strings.

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// semver builtin - semantic versioning utilities
pub struct Semver;

/// Parsed semver components
struct SemverParts {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Option<String>,
}

fn parse_semver(input: &str) -> Option<SemverParts> {
    let s = input.trim().strip_prefix('v').unwrap_or(input.trim());
    let (version_part, pre) = if let Some(idx) = s.find('-') {
        (&s[..idx], Some(s[idx + 1..].to_string()))
    } else {
        (s, None)
    };

    let parts: Vec<&str> = version_part.split('.').collect();
    if parts.len() != 3 {
        return None;
    }

    let major = parts[0].parse::<u64>().ok()?;
    let minor = parts[1].parse::<u64>().ok()?;
    let patch = parts[2].parse::<u64>().ok()?;

    Some(SemverParts {
        major,
        minor,
        patch,
        pre,
    })
}

/// Compare two semver strings. Returns -1, 0, or 1.
fn cmp_semver(a: &str, b: &str) -> Option<i32> {
    let a = parse_semver(a)?;
    let b = parse_semver(b)?;

    let ord = a
        .major
        .cmp(&b.major)
        .then(a.minor.cmp(&b.minor))
        .then(a.patch.cmp(&b.patch))
        .then_with(|| match (&a.pre, &b.pre) {
            (None, None) => std::cmp::Ordering::Equal,
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(ap), Some(bp)) => ap.cmp(bp),
        });

    Some(match ord {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    })
}

#[async_trait]
impl Builtin for Semver {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "semver: usage: semver <subcommand> [args...]\nSubcommands: compare, gt, lt, eq, gte, lte, parse, bump, valid, sort\n".to_string(),
                1,
            ));
        }

        let subcmd = ctx.args[0].as_str();
        let rest = &ctx.args[1..];

        match subcmd {
            "compare" => {
                if rest.len() != 2 {
                    return Ok(ExecResult::err(
                        "semver: compare requires two version arguments\n".to_string(),
                        1,
                    ));
                }
                match cmp_semver(&rest[0], &rest[1]) {
                    Some(v) => Ok(ExecResult::ok(format!("{v}\n"))),
                    None => Ok(ExecResult::err("semver: invalid version\n".to_string(), 1)),
                }
            }
            "gt" | "lt" | "eq" | "gte" | "lte" => {
                if rest.len() != 2 {
                    return Ok(ExecResult::err(
                        format!("semver: {subcmd} requires two version arguments\n"),
                        1,
                    ));
                }
                match cmp_semver(&rest[0], &rest[1]) {
                    Some(c) => {
                        let result = match subcmd {
                            "gt" => c > 0,
                            "lt" => c < 0,
                            "eq" => c == 0,
                            "gte" => c >= 0,
                            "lte" => c <= 0,
                            _ => unreachable!(),
                        };
                        if result {
                            Ok(ExecResult::with_code("", 0))
                        } else {
                            Ok(ExecResult::with_code("", 1))
                        }
                    }
                    None => Ok(ExecResult::err("semver: invalid version\n".to_string(), 1)),
                }
            }
            "parse" => {
                if rest.len() != 1 {
                    return Ok(ExecResult::err(
                        "semver: parse requires one version argument\n".to_string(),
                        1,
                    ));
                }
                match parse_semver(&rest[0]) {
                    Some(v) => {
                        let mut out =
                            format!("major={}\nminor={}\npatch={}\n", v.major, v.minor, v.patch);
                        if let Some(ref pre) = v.pre {
                            out.push_str(&format!("pre={pre}\n"));
                        }
                        Ok(ExecResult::ok(out))
                    }
                    None => Ok(ExecResult::err("semver: invalid version\n".to_string(), 1)),
                }
            }
            "bump" => {
                if rest.len() != 2 {
                    return Ok(ExecResult::err(
                        "semver: bump requires <major|minor|patch> <version>\n".to_string(),
                        1,
                    ));
                }
                let component = rest[0].as_str();
                match parse_semver(&rest[1]) {
                    Some(v) => {
                        let (major, minor, patch) = match component {
                            "major" => (v.major + 1, 0, 0),
                            "minor" => (v.major, v.minor + 1, 0),
                            "patch" => (v.major, v.minor, v.patch + 1),
                            _ => {
                                return Ok(ExecResult::err(
                                    format!(
                                        "semver: bump: unknown component '{component}', use major/minor/patch\n"
                                    ),
                                    1,
                                ));
                            }
                        };
                        Ok(ExecResult::ok(format!("{major}.{minor}.{patch}\n")))
                    }
                    None => Ok(ExecResult::err("semver: invalid version\n".to_string(), 1)),
                }
            }
            "valid" => {
                if rest.len() != 1 {
                    return Ok(ExecResult::err(
                        "semver: valid requires one version argument\n".to_string(),
                        1,
                    ));
                }
                if parse_semver(&rest[0]).is_some() {
                    Ok(ExecResult::with_code("", 0))
                } else {
                    Ok(ExecResult::with_code("", 1))
                }
            }
            "sort" => {
                let input = ctx.stdin.unwrap_or("");
                let mut versions: Vec<&str> =
                    input.lines().filter(|l| !l.trim().is_empty()).collect();
                versions.sort_by(|a, b| {
                    cmp_semver(a, b)
                        .map(|c| match c {
                            -1 => std::cmp::Ordering::Less,
                            1 => std::cmp::Ordering::Greater,
                            _ => std::cmp::Ordering::Equal,
                        })
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                let mut out = String::new();
                for v in versions {
                    out.push_str(v);
                    out.push('\n');
                }
                Ok(ExecResult::ok(out))
            }
            _ => Ok(ExecResult::err(
                format!("semver: unknown subcommand '{subcmd}'\n"),
                1,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run(args: &[&str], stdin: Option<&str>) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            shell: None,
        };
        Semver.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_compare_equal() {
        let r = run(&["compare", "1.2.3", "1.2.3"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "0");
    }

    #[tokio::test]
    async fn test_compare_greater() {
        let r = run(&["compare", "2.0.0", "1.9.9"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "1");
    }

    #[tokio::test]
    async fn test_compare_less() {
        let r = run(&["compare", "1.0.0", "1.0.1"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "-1");
    }

    #[tokio::test]
    async fn test_gt() {
        let r = run(&["gt", "2.0.0", "1.0.0"], None).await;
        assert_eq!(r.exit_code, 0);
        let r = run(&["gt", "1.0.0", "2.0.0"], None).await;
        assert_eq!(r.exit_code, 1);
    }

    #[tokio::test]
    async fn test_parse() {
        let r = run(&["parse", "v1.2.3-beta"], None).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("major=1"));
        assert!(r.stdout.contains("minor=2"));
        assert!(r.stdout.contains("patch=3"));
        assert!(r.stdout.contains("pre=beta"));
    }

    #[tokio::test]
    async fn test_bump_major() {
        let r = run(&["bump", "major", "1.2.3"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "2.0.0");
    }

    #[tokio::test]
    async fn test_bump_minor() {
        let r = run(&["bump", "minor", "1.2.3"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "1.3.0");
    }

    #[tokio::test]
    async fn test_bump_patch() {
        let r = run(&["bump", "patch", "1.2.3"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "1.2.4");
    }

    #[tokio::test]
    async fn test_valid() {
        let r = run(&["valid", "1.2.3"], None).await;
        assert_eq!(r.exit_code, 0);
        let r = run(&["valid", "not-a-version"], None).await;
        assert_eq!(r.exit_code, 1);
    }

    #[tokio::test]
    async fn test_sort() {
        let r = run(&["sort"], Some("3.0.0\n1.0.0\n2.0.0\n")).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "1.0.0\n2.0.0\n3.0.0\n");
    }

    #[tokio::test]
    async fn test_prerelease_compare() {
        // pre-release is less than release
        let r = run(&["compare", "1.0.0-alpha", "1.0.0"], None).await;
        assert_eq!(r.stdout.trim(), "-1");
    }

    #[tokio::test]
    async fn test_v_prefix() {
        let r = run(&["compare", "v1.0.0", "1.0.0"], None).await;
        assert_eq!(r.stdout.trim(), "0");
    }

    #[tokio::test]
    async fn test_no_args() {
        let r = run(&[], None).await;
        assert_eq!(r.exit_code, 1);
    }

    #[tokio::test]
    async fn test_invalid_version() {
        let r = run(&["compare", "abc", "1.0.0"], None).await;
        assert_eq!(r.exit_code, 1);
    }
}
