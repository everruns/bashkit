# SSH Support

## Status

Phase 1: Implemented — Handler trait, allowlist, ssh/scp/sftp builtins

## Decision

SSH/SCP/SFTP builtins behind the `ssh` feature flag (pulls in the
`russh`-based default transport). Same opt-in pattern as `git` and
`http_client`: disabled unless `SshConfig` is set via builder.

### Supported Commands

Phase 1 (implemented): `ssh [user@]host command...` (with `-i` keyfile from
VFS, `-p` port), `scp` to/from remote, `sftp [user@]host` (heredoc/pipe mode).

Phase 2 (future): interactive `ssh` sessions via heredoc, port forwarding
(`-L`/`-R`), agent forwarding (`-A`).

### Architecture

Follows the HTTP pattern: trait + allowlist + default implementation.

```
ssh/scp/sftp builtins (parse args, validate host, delegate)
  → SshClient (holds SshConfig + SshHandler, enforces allowlist, session pool)
    → SshHandler trait (pluggable: default russh impl; custom mock/proxy/log)
  SshAllowlist: host glob patterns, port restrictions, default-deny
```

`SshHandler` trait (`exec`/`upload`/`download`): see
`crates/bashkit/src/builtins/ssh/handler.rs` / rustdoc. Custom handlers via
`Bash::builder().ssh_handler(...)`.

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

`SshConfig` (see `crates/bashkit/src/builtins/ssh/config.rs` / rustdoc):
`allow(pattern)`, `allow_port(n)` (default: 22), `default_user(u)`,
`timeout(d)`, `max_response_bytes(n)`, `max_sessions(n)`. Set via
`Bash::builder().ssh(config)`.

### Allowlist Patterns

- Exact host: `db.abc123.supabase.co`
- Wildcard subdomain: `*.supabase.co`
- IP address: `192.168.1.100`
- Patterns apply to the allowed-ports list; no scheme (always SSH)
