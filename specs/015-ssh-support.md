# 015: SSH Support

## Status

Phase 1: Implemented — Handler trait, allowlist, ssh/scp/sftp builtins

## Decision

Bashkit provides SSH/SCP/SFTP builtins via the `ssh` feature flag.
Follows the same opt-in pattern as `git` and `http_client`.

### Feature Flag

Enable with:
```toml
[dependencies]
bashkit = { version = "0.1", features = ["ssh"] }
```

Pulls in the `russh`-based default transport implementation.

### Configuration

```rust
use bashkit::{Bash, SshConfig};

let bash = Bash::builder()
    .ssh(SshConfig::new()
        .allow("db.abc123.supabase.co")
        .allow("*.example.com")
        .default_user("root")
        .timeout(Duration::from_secs(30)))
    .build();
```

### Supported Commands

#### Phase 1 — Command Execution

| Command | Description |
|---------|-------------|
| `ssh [user@]host command...` | Execute command on remote host |
| `ssh -i keyfile [user@]host command...` | With identity file (from VFS) |
| `ssh -p port [user@]host command...` | Custom port |
| `scp source [user@]host:dest` | Copy file to remote |
| `scp [user@]host:source dest` | Copy file from remote |
| `sftp [user@]host` | Interactive-ish file transfer (heredoc/pipe mode) |

#### Phase 2 — Interactive Sessions (Future)

| Command | Description |
|---------|-------------|
| `ssh [user@]host` (no command) | Interactive session via heredoc |
| Port forwarding | `-L`, `-R` tunnel support |
| Agent forwarding | `-A` SSH agent support |

### Architecture

Follows the HTTP pattern: trait + allowlist + default implementation.

```
┌─────────────────────────────────────┐
│  ssh/scp/sftp builtins              │
│  - Parse CLI args                   │
│  - Validate host against allowlist  │
│  - Delegate to SshClient            │
├─────────────────────────────────────┤
│  SshClient                          │
│  - Holds SshConfig + SshHandler     │
│  - Enforces allowlist before calls  │
│  - Manages session pool             │
├─────────────────────────────────────┤
│  SshHandler trait (pluggable)       │
│  - Default: russh-based impl        │
│  - Custom: mock, proxy, log, etc.   │
├─────────────────────────────────────┤
│  SshAllowlist                       │
│  - Host patterns with glob support  │
│  - Port restrictions                │
│  - Default-deny                     │
└─────────────────────────────────────┘
```

### Handler Trait

```rust
#[async_trait]
pub trait SshHandler: Send + Sync {
    /// Execute a command on a remote host.
    async fn exec(
        &self,
        target: &SshTarget,
        command: &str,
    ) -> std::result::Result<SshOutput, String>;

    /// Upload a file to a remote host (scp/sftp put).
    async fn upload(
        &self,
        target: &SshTarget,
        remote_path: &str,
        content: &[u8],
        mode: u32,
    ) -> std::result::Result<(), String>;

    /// Download a file from a remote host (scp/sftp get).
    async fn download(
        &self,
        target: &SshTarget,
        remote_path: &str,
    ) -> std::result::Result<Vec<u8>, String>;
}
```

### Security Model

- **Disabled by default**: SSH requires explicit `SshConfig` via builder
- **Host allowlist**: Only allowed hosts can be connected to (default-deny)
- **No credential leakage**: Keys read from VFS only, never from host `~/.ssh/`
- **Resource limits**: Max concurrent sessions, connection timeout, response size
- **No agent forwarding by default**: Must be explicitly enabled
- **Port restrictions**: Configurable allowed ports (default: 22)

### Threat IDs

| ID | Threat | Mitigation |
|----|--------|-----------|
| TM-SSH-001 | Unauthorized host access | Host allowlist (default-deny) |
| TM-SSH-002 | Credential leakage | Keys from VFS only, no host ~/.ssh/ |
| TM-SSH-003 | Session exhaustion | Max concurrent sessions limit |
| TM-SSH-004 | Response size bomb | Max response bytes limit |
| TM-SSH-005 | Connection hang | Connect + read timeouts |
| TM-SSH-006 | Host key MITM | Configurable host key verification |
| TM-SSH-007 | Port scanning | Port allowlist |
| TM-SSH-008 | Command injection via args | Shell-escape remote commands |

### Builder API

```rust
Bash::builder()
    .ssh(SshConfig::new()
        .allow("*.supabase.co")          // Host glob pattern
        .allow_port(22)                   // Allowed ports (default: 22)
        .allow_port(2222)
        .default_user("root")
        .timeout(Duration::from_secs(30))
        .max_response_bytes(10_000_000)   // 10MB
        .max_sessions(5))
    .ssh_handler(Box::new(custom_handler)) // Optional custom handler
    .build()
```

### Allowlist Patterns

- Exact host: `db.abc123.supabase.co`
- Wildcard subdomain: `*.supabase.co`
- IP address: `192.168.1.100`
- With port override: patterns apply to allowed ports list

No scheme needed (always SSH protocol).
