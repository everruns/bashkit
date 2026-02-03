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
│   ├── compatibility.md      # Bash compatibility scorecard (reference)
│   ├── custom_builtins.md    # Guide for extending BashKit
│   └── (future docs...)
└── src/
    └── lib.rs
        ├── //! crate docs with links to guides and references
        └── pub mod custom_builtins_guide {}      # Tutorial-style guide
            pub mod compatibility_reference {}    # Dense reference/scorecard
```

### Guides vs References

- **Guides** (`*_guide`): Tutorial-style docs with examples, step-by-step instructions
- **References** (`*_reference`): Dense lookup tables, compatibility scorecards, quick status

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

Add "Guides" and "References" sections to main crate documentation:

```rust
//! # Guides
//!
//! - [`custom_builtins_guide`] - Creating custom builtins
//!
//! # References
//!
//! - [`compatibility_reference`] - Bash compatibility scorecard
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

## Verification

- `cargo doc` builds without errors
- Links resolve correctly in generated docs
- Markdown renders properly in rustdoc
