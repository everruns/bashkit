# Git

Bashkit ships a sandboxed `git` builtin behind the `git` feature flag. Every
operation runs against the in-memory [virtual filesystem](filesystem.md) — there
is no host `~/.gitconfig`, no host credentials, and no network unless you
explicitly allow a remote. Scripts get a familiar `git` workflow; the host stays
isolated.

## Quick start

```rust,no_run
use bashkit::{Bash, GitConfig};

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let mut bash = Bash::builder()
    .git(GitConfig::new().author("Ada Lovelace", "ada@example.com"))
    .build();

bash.exec("git init").await?;
bash.exec("echo 'hello' > README.md").await?;
bash.exec("git add README.md").await?;
bash.exec("git commit -m 'first commit'").await?;
bash.exec("git log -n 1").await?;
# Ok(())
# }
```

The commit identity comes from `GitConfig`, not from the host — see
[TM-GIT-002](security.md). With no author configured, Bashkit uses a neutral
virtual identity rather than reading any host config.

## Supported commands

| Area | Commands |
|------|----------|
| **Local** | `init`, `config`, `add`, `commit -m`, `status`, `log [-n N]` |
| **Branching** | `branch [-d]`, `checkout [-b]`, `diff` (simplified), `reset [--soft\|--mixed\|--hard]` |
| **Remotes** | `remote [-v]`, `remote add/remove` (fully functional) |
| **Remote transfer** | `clone`, `push`, `pull`, `fetch` — validate the URL against the allowlist, then return virtual-mode messages (no network in VFS-only mode) |

`merge`, `rebase`, and `stash` are not yet implemented.

```bash
git init
git config user.name "Ada"
echo "data" > file.txt
git add .
git commit -m "add file"
git status
git checkout -b feature
git log -n 5
```

## Remote operations

Remote transfer commands operate in **virtual mode**: the URL is validated, but
no bytes cross the network. Remotes must be allowlisted explicitly, and only
HTTPS is accepted — no SSH and no `git://`.

```rust,no_run
use bashkit::GitConfig;

let config = GitConfig::new()
    .author("Ada Lovelace", "ada@example.com")
    .allow_remote("https://github.com/everruns/*");
```

A `git push https://evil.example/repo.git` is rejected before any URL parsing
side effects, because the host is not in the remote allowlist.

## Storage model

Phase 1 uses a simplified on-VFS layout (`.git/HEAD`, `.git/config`,
`.git/index`, `.git/commits`, `.git/refs/heads/<branch>`). This gives full VFS
isolation and correct user-facing behaviour today, and is the foundation for a
future [`gix`](https://crates.io/crates/gix) backend with real object format and
networked remotes.

## Security

- **No host config or credentials** — identity is virtual and configurable
  (TM-GIT-002, TM-GIT-004).
- **No repository escape** — every path stays inside the VFS (TM-GIT-005).
- **Remote allowlist** — HTTPS-only, explicit patterns, no implicit network
  (TM-GIT-003).
- **DoS caps** — object count, history depth, and pack size are bounded by the
  filesystem and `log` limits (TM-GIT-007/008/009).

See the [security model](security.md) for the sandbox boundaries that apply to
every builtin.

## See also

- [Virtual filesystem](filesystem.md) — where the repository lives.
- [Security](security.md) — sandbox boundaries and the `TM-GIT-*` threats.
- Spec: [`specs/git-support.md`](https://github.com/everruns/bashkit/blob/main/specs/git-support.md).
