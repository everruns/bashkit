//! Compose a generic build sandbox from independent filesystems.

use bashkit::{Bash, FileSystem, InMemoryFs, NamespaceFs};
use std::path::Path;
use std::sync::Arc;

#[tokio::main]
async fn main() -> bashkit::Result<()> {
    let source = Arc::new(InMemoryFs::new());
    source
        .write_file(Path::new("/main.rs"), b"fn main() {}\n")
        .await?;

    let build = Arc::new(InMemoryFs::new());
    let cache = Arc::new(InMemoryFs::new());
    let temporary = Arc::new(InMemoryFs::new());
    let namespace = NamespaceFs::builder()
        .mount_readonly("/src", source)?
        .mount_readwrite("/build", build.clone())?
        .mount_readwrite("/cache", cache.clone())?
        .mount_readwrite("/tmp", temporary)?
        .build();

    let mut bash = Bash::builder().fs(Arc::new(namespace)).build();
    let result = bash
        .exec(
            "cat /src/main.rs; printf artifact > /build/app; \
             printf cached > /cache/index; printf scratch > /tmp/work",
        )
        .await?;
    assert_eq!(result.stdout, "fn main() {}\n");
    assert_eq!(build.read_file(Path::new("/app")).await?, b"artifact");
    assert_eq!(cache.read_file(Path::new("/index")).await?, b"cached");

    let denied = bash.exec("printf changed > /src/main.rs").await?;
    assert_ne!(denied.exit_code, 0);
    println!(
        "{}build, cache, and tmp are writable; src is read-only",
        result.stdout
    );
    Ok(())
}
