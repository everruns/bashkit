# SSH Support

Bashkit provides `ssh`, `scp`, and `sftp` builtins for remote command execution
and file transfer over SSH. The default transport uses [russh](https://crates.io/crates/russh).

**See also:** [`specs/ssh-support.md`][spec]

## Quick Start

```rust,no_run
use bashkit::{Bash, SshConfig};

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let mut bash = Bash::builder()
    .ssh(SshConfig::new().allow("supabase.sh"))
    .build();

let result = bash.exec("ssh supabase.sh").await?;
# Ok(())
# }
```

## Usage

```bash
# Remote command
ssh host.example.com 'uname -a'

# Heredoc
ssh host.example.com <<'EOF'
psql -c 'SELECT version()'
EOF

# Shell session (TUI services like supabase.sh)
ssh supabase.sh

# SCP
scp local.txt host.example.com:/remote/path.txt
scp host.example.com:/remote/file.txt local.txt

# SFTP (heredoc/pipe mode)
sftp host.example.com <<'EOF'
put /tmp/data.csv /var/import/data.csv
get /var/export/report.csv /tmp/report.csv
ls /var/import
EOF
```

## Configuration

```rust,no_run
use bashkit::SshConfig;
use std::time::Duration;

let config = SshConfig::new()
    .allow("*.supabase.co")           // wildcard subdomain
    .allow("bastion.example.com")     // exact host
    .allow_port(2222)                 // additional port (default: 22 only)
    .default_user("deploy")           // when no user@ prefix
    .timeout(Duration::from_secs(30)) // connection timeout
    .max_response_bytes(10_000_000)   // max output size
    .max_sessions(5);                 // concurrent session limit
```

## Authentication

Tried in order: none (public services) → public key (`-i` flag or `default_private_key()`) → password (`default_password()`).

## Security

- Default-deny host allowlist with glob patterns and port restrictions
- Keys read from VFS only, never host `~/.ssh/`
- Remote paths shell-escaped (TM-SSH-008)
- Response size and session count limits

[spec]: https://github.com/everruns/bashkit/blob/main/specs/ssh-support.md
