# Builtins and Shell Support

Use this when the user asks what Bashkit can run.

## Shell Features

Bashkit supports common Bash/POSIX shell features:

- variables and parameter expansion
- command substitution
- arithmetic expansion
- pipelines and redirections
- `if`, `for`, `while`, `until`, `case`, `select`
- functions
- indexed and associative arrays
- brace expansion and glob expansion
- here documents and here strings
- process substitution
- background execution with `wait`
- `set` and `shopt`
- alias expansion
- traps
- `[[ ]]` conditionals with regex matching

If the user needs exact Bash parity, check the compatibility docs before answering.

## Builtin Categories

The project tracks 156 built-in commands (generated specs/status/builtins.json). Important groups:

- Core: `echo`, `printf`, `cat`, `nl`, `read`, `mapfile`, `readarray`
- Navigation: `cd`, `pwd`, `ls`, `tree`, `find`, `pushd`, `popd`, `dirs`
- Flow control: `true`, `false`, `exit`, `return`, `break`, `continue`, `test`, `[`
- Variables/shell: `export`, `set`, `unset`, `local`, `source`, `.`, `eval`, `declare`, `alias`, `trap`, `getopts`, `help`
- Text: `grep`, `rg`, `sed`, `awk`, `jq`, `head`, `tail`, `sort`, `uniq`, `cut`, `tr`, `wc`, `diff`, `seq`, `expr`
- Files: `mkdir`, `mktemp`, `rm`, `cp`, `mv`, `touch`, `chmod`, `ln`, `realpath`, `split`
- Archives: `tar`, `gzip`, `gunzip`, `zip`, `unzip`
- Checksums/bytes: `md5sum`, `sha1sum`, `sha256sum`, `od`, `xxd`, `hexdump`, `base64`
- Data formats: `csv`, `json`, `yaml`, `tomlq`, `template`, `envsubst`
- Network: `curl`, `wget`, `http`
- DevOps: `assert`, `dotenv`, `glob`, `log`, `retry`, `semver`, `verify`, `parallel`, `patch`
- Experimental/features: `git`, `ssh`, `scp`, `sftp`, `python`, `python3`, `ts`, `typescript`, `node`, `deno`, `bun`, `sqlite`, `sqlite3`

## Security-Gated Features

- `curl`, `wget`, and `http` need HTTP/network allowlist configuration.
- Host filesystem access needs an explicit mount.
- `python`, `typescript`, `sqlite`, `git`, `ssh`, and `realfs` availability depends on package/build features and runtime configuration.

## Not Implemented in CLI REPL

- job control: `bg`, `fg`, `jobs`
- history expansion: `!!`, `!N`
- persistent history file
- `exec`

## Reference

- README builtins table: https://github.com/everruns/bashkit/blob/main/README.md
- Compatibility scorecard: https://docs.rs/bashkit/latest/bashkit/compatibility_scorecard/
- CLI docs: https://github.com/everruns/bashkit/blob/main/docs/cli.md
