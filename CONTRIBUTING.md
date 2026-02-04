# Contributing to BashKit

Thanks for your interest in contributing to BashKit!

## Setup

```bash
# Clone
git clone https://github.com/everruns/bashkit.git
cd bashkit

# Install just (task runner)
cargo install just

# Build
just build

# Test
just test
```

## Development Workflow

1. Fork the repo
2. Create a feature branch
3. Make changes
4. Run pre-PR checks: `just pre-pr`
5. Submit a pull request

## Commands

```bash
just --list       # Show all commands
just build        # Build all crates
just test         # Run all tests
just fmt          # Format code (auto-fix)
just check        # fmt + clippy + test (checks only)
just pre-pr       # Full pre-PR validation
```

## Code Style

- Format with `cargo fmt`
- Lint with `cargo clippy -- -D warnings`
- License check: `cargo deny check`

## Commits

Follow [Conventional Commits](https://www.conventionalcommits.org):

```
feat(parser): add brace expansion support
fix(awk): handle regex in gsub correctly
docs: update compatibility scorecard
test: add array edge case tests
```

## Adding Features

1. Check if the feature is documented in `specs/`
2. Add spec tests in `crates/bashkit/tests/spec_cases/`
3. Implement the feature
4. Update `crates/bashkit/docs/compatibility.md` if applicable
5. Update `specs/009-implementation-status.md` if removing a limitation

## Spec Test Format

Tests live in `.test.sh` files:

```sh
### test_name
# Optional description
echo hello world
### expect
hello world
### end

### skipped_test
### skip: reason for skipping
command
### expect
expected
### end
```

## Pull Request Checklist

- [ ] `just pre-pr` passes
- [ ] Rebased on main
- [ ] Specs updated if behavior changes
- [ ] CI green

## Architecture

See `specs/` for design documents:

- `001-architecture.md` - Overall design
- `002-parser.md` - Parser/lexer details
- `003-vfs.md` - Virtual filesystem
- `004-testing.md` - Testing strategy

## Questions?

Open an issue at https://github.com/everruns/bashkit/issues
