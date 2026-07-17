//! Rebase a source subtree and place a writable mount inside it.

use bashkit::{Bash, FileSystem, InMemoryFs, NamespaceFs};
use std::path::Path;
use std::sync::Arc;

#[tokio::main]
async fn main() -> bashkit::Result<()> {
    let repository = Arc::new(InMemoryFs::new());
    repository
        .mkdir(Path::new("/repo/src/generated"), true)
        .await?;
    repository
        .write_file(
            Path::new("/repo/src/lib.rs"),
            b"pub fn answer() -> u8 { 42 }\n",
        )
        .await?;
    repository
        .write_file(Path::new("/repo/private.txt"), b"not mounted")
        .await?;

    let generated = Arc::new(InMemoryFs::new());
    let namespace = NamespaceFs::builder()
        .mount_readonly_from("/workspace", repository, "/repo/src")?
        .mount_readwrite("/workspace/generated", generated.clone())?
        .build();

    let mut bash = Bash::builder().fs(Arc::new(namespace)).build();
    let result = bash
        .exec("cat /workspace/lib.rs; printf generated > /workspace/generated/schema.rs")
        .await?;
    assert_eq!(result.stdout, "pub fn answer() -> u8 { 42 }\n");
    assert_eq!(
        generated.read_file(Path::new("/schema.rs")).await?,
        b"generated"
    );
    assert!(!bash.fs().exists(Path::new("/private.txt")).await?);
    println!(
        "{0}nested writable override created schema.rs",
        result.stdout
    );
    Ok(())
}
