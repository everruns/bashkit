# 008: Documentation Approach

## Decision

Use `include_str!` macro to embed external markdown files into rustdoc as documentation modules.

## Rationale

1. **Single source of truth**: Markdown files in `crates/bashkit/docs/` are the canonical source
2. **Dual visibility**: Same content visible on GitHub and in docs.rs/rustdoc
3. **No duplication**: Avoids maintaining separate docs for different platforms
4. **Cross-linking**: rustdoc links connect guides to API types

## Structure

```
crates/bashkit/
├── docs/
│   ├── compatibility.md      # Bash compatibility scorecard
│   ├── custom_builtins.md    # Guide for extending Bashkit
│   └── (future docs...)
└── src/
    └── lib.rs
        ├── //! crate docs with links to guides
        └── pub mod custom_builtins_guide {}
            pub mod compatibility_scorecard {}
```

Note: Docs live inside `crates/bashkit/docs/` to ensure they are included in
the published crate package. This allows `include_str!` to work correctly
when the crate is built from crates.io.

## Implementation

### Doc Modules

```rust
/// Brief description and cross-links
#[doc = include_str!("../docs/guide_name.md")]
pub mod guide_name {}
```

- Module is empty (just `{}`), content comes from markdown
- Add `///` doc comments above for rustdoc cross-links
- Reference related types with `[`TypeName`]` syntax

### Cross-links in Markdown

Add "See also" section at top of each markdown file:

```markdown
**See also:**
- [API Documentation](https://docs.rs/bashkit) - Full API reference
- [Other Guide](./other_guide.md) - Brief description
```

### Crate Docs

Add "Guides" section and link to scorecard in main crate documentation:

```rust
//! # Shell Features
//! ...
//! - [`compatibility_scorecard`] - Full compatibility status
//!
//! # Quick Start
//! ...
//!
//! # Guides
//!
//! - [`custom_builtins_guide`] - Creating custom builtins
```

## Adding New Guides

1. Create `crates/bashkit/docs/new_guide.md` with content
2. Add "See also" links to related guides
3. Add doc module in `lib.rs`:
   ```rust
   /// Brief description
   #[doc = include_str!("../docs/new_guide.md")]
   pub mod new_guide {}
   ```
4. Add link in crate docs `# Guides` section
5. Run `cargo doc --open` to verify

## Code Examples

Rust code examples in guides are compiled and tested by `cargo test --doc`.

### Fencing rules

| Fence | When to use |
|-------|-------------|
| `` ```rust `` | Complete examples using only bashkit types — tested |
| `` ```rust,no_run `` | Complete examples that compile but shouldn't execute |
| `` ```rust,ignore `` | Uses external crates (sqlx, reqwest, tracing-subscriber) or feature-gated APIs in non-gated modules |

### Making examples testable

Use `# ` (hash-space) prefix to hide boilerplate lines from rendered docs while
keeping them in the compiled test:

````markdown
```rust
# use bashkit::Bash;
# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let mut bash = Bash::new();
let result = bash.exec("echo hello").await?;
assert_eq!(result.stdout, "hello\n");
# Ok(())
# }
```
````

### Feature-gated modules

Doc modules behind `#[cfg(feature = "...")]` (e.g., `python_guide`, `logging_guide`)
can use feature-gated APIs freely — their tests only run when the feature is enabled.

Non-gated modules (e.g., `threat_model`, `compatibility_scorecard`) must NOT use
feature-gated APIs in tested examples. Use `rust,ignore` for those.

## Verification

- `cargo doc` builds without errors
- `cargo test --doc --all-features` passes
- Links resolve correctly in generated docs
- Markdown renders properly in rustdoc
