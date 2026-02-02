# 008: Documentation Approach

## Decision

Use `include_str!` macro to embed external markdown files into rustdoc as documentation modules.

## Rationale

1. **Single source of truth**: Markdown files in `docs/` are the canonical source
2. **Dual visibility**: Same content visible on GitHub and in docs.rs/rustdoc
3. **No duplication**: Avoids maintaining separate docs for different platforms
4. **Cross-linking**: rustdoc links connect guides to API types

## Structure

```
docs/
├── compatibility.md      # Bash feature compatibility reference
├── custom_builtins.md    # Guide for extending BashKit
└── (future guides...)

crates/bashkit/src/lib.rs
├── //! crate docs with links to guides
└── pub mod custom_builtins_guide {}   # Empty module with include_str!
    pub mod compatibility_guide {}
```

## Implementation

### Doc Modules

```rust
/// Brief description and cross-links
#[doc = include_str!("../../../docs/guide_name.md")]
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

Add "Guides" section to main crate documentation:

```rust
//! # Guides
//!
//! - [`custom_builtins_guide`] - Creating custom builtins
//! - [`compatibility_guide`] - Full bash compatibility reference
```

## Adding New Guides

1. Create `docs/new_guide.md` with content
2. Add "See also" links to related guides
3. Add doc module in `lib.rs`:
   ```rust
   /// Brief description
   #[doc = include_str!("../../../docs/new_guide.md")]
   pub mod new_guide {}
   ```
4. Add link in crate docs `# Guides` section
5. Run `cargo doc --open` to verify

## Verification

- `cargo doc` builds without errors
- Links resolve correctly in generated docs
- Markdown renders properly in rustdoc
