# Third-Party Licenses

This directory contains license texts for projects that have influenced
BashKit's design or whose test case formats have inspired our testing approach.

## Important Notes

1. **BashKit is an independent implementation.** No source code has been copied
   from any of these projects. All Rust code in BashKit is original.

2. **Test cases are original.** While our testing methodology was inspired by
   projects like Oils, the actual test cases are written specifically for
   BashKit.

3. **Dependencies are via Cargo.** Rust dependencies (like jaq for jq support)
   are included via standard Cargo dependency management, not by copying source.

## License Files

| File | Projects | License Type |
|------|----------|--------------|
| `APACHE-2.0.txt` | just-bash, Oils | Apache License 2.0 |
| `MIT.txt` | jq, jaq crates, most dependencies | MIT License |
| `LUCENT.txt` | One True AWK | Lucent Public License |

## Full Dependency Licenses

For a complete list of all Rust dependencies and their licenses, run:

```bash
cargo tree --format "{p} {l}"
```

Or use cargo-deny for license compliance checking:

```bash
cargo deny check licenses
```

## Contact

If you have questions about licensing, please open an issue at:
https://github.com/everruns/bashkit/issues
