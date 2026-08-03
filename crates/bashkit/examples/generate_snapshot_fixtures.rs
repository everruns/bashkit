//! Regenerate the golden snapshot fixtures under
//! `crates/bashkit/tests/fixtures/snapshots/`.
//!
//! Run with `cargo run -p bashkit --example generate_snapshot_fixtures` when a
//! **new** format version ships. Existing fixtures are never regenerated —
//! that would defeat their purpose, which is to prove that today's build still
//! reads bytes written by an older one. See
//! `knowledge/foundations/snapshot-history.md`.

use bashkit::{Bash, SnapshotOptions};
use std::path::Path;

/// State every fixture captures, so the corpus test can assert the same
/// expectations against any version.
pub const SETUP: &str = r#"
GOLDEN=fixture
export EXPORTED=yes
arr=(alpha beta gamma)
greet() { echo "hi $1"; }
mkdir -p /golden/nested
echo 'plain text' > /golden/text.txt
echo 'AAH//n+AAEFCCg0=' | base64 -d > /golden/blob.bin
ln -s /golden/text.txt /golden/link
cd /golden
"#;

#[tokio::main]
async fn main() -> bashkit::Result<()> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/snapshots");
    std::fs::create_dir_all(&dir).map_err(|e| bashkit::Error::Internal(e.to_string()))?;

    let mut bash = Bash::new();
    bash.exec(SETUP).await?;

    write(
        &dir.join("v1.snapshot"),
        bash.snapshot_state(SnapshotOptions::default()).to_bytes()?,
    )?;
    write(&dir.join("v2.snapshot"), bash.snapshot()?)?;
    Ok(())
}

fn write(path: &Path, bytes: Vec<u8>) -> bashkit::Result<()> {
    if path.exists() {
        println!("skipping existing fixture {}", path.display());
        return Ok(());
    }
    std::fs::write(path, &bytes).map_err(|e| bashkit::Error::Internal(e.to_string()))?;
    println!("wrote {} ({} bytes)", path.display(), bytes.len());
    Ok(())
}
