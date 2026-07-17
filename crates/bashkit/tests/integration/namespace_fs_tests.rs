use bashkit::{
    Error, FileSystem, FileSystemExt, FileType, InMemoryFs, NamespaceAccess, NamespaceFs,
    ReadOnlyFs,
};
use std::io::ErrorKind;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

async fn source_with(path: &str, content: &[u8]) -> Arc<InMemoryFs> {
    let fs = Arc::new(InMemoryFs::new());
    let parent = Path::new(path).parent().unwrap();
    fs.mkdir(parent, true).await.unwrap();
    fs.write_file(Path::new(path), content).await.unwrap();
    fs
}

#[tokio::test]
async fn namespace_mounts_arbitrary_filesystems_with_longest_prefix_precedence() {
    let parent = source_with("/parent.txt", b"parent").await;
    parent.mkdir(Path::new("/nested"), false).await.unwrap();
    parent
        .write_file(Path::new("/nested/hidden.txt"), b"hidden")
        .await
        .unwrap();
    let nested = source_with("/visible.txt", b"nested").await;

    let namespace = NamespaceFs::builder()
        .mount_readwrite("/workspace", parent.clone())
        .unwrap()
        .mount_readwrite("/workspace/nested", nested.clone())
        .unwrap()
        .build();

    assert_eq!(
        namespace
            .read_file(Path::new("/workspace/parent.txt"))
            .await
            .unwrap(),
        b"parent"
    );
    assert_eq!(
        namespace
            .read_file(Path::new("/workspace/nested/visible.txt"))
            .await
            .unwrap(),
        b"nested"
    );
    assert!(
        !namespace
            .exists(Path::new("/workspace/nested/hidden.txt"))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn namespace_lists_synthetic_ancestors_and_mount_points_consistently() {
    let source = source_with("/project/file.txt", b"hello").await;
    let namespace = NamespaceFs::builder()
        .mount_from(
            "/sandbox/work/src",
            source,
            "/project",
            NamespaceAccess::ReadOnly,
        )
        .unwrap()
        .build();

    for path in ["/", "/sandbox", "/sandbox/work", "/sandbox/work/src"] {
        let metadata = namespace.stat(Path::new(path)).await.unwrap();
        assert_eq!(metadata.file_type, FileType::Directory, "{path}");
        assert!(namespace.exists(Path::new(path)).await.unwrap(), "{path}");
    }

    let root = namespace.read_dir(Path::new("/")).await.unwrap();
    assert_eq!(
        root.iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["sandbox"]
    );

    let work = namespace
        .read_dir(Path::new("/sandbox/work"))
        .await
        .unwrap();
    let src = work.iter().find(|entry| entry.name == "src").unwrap();
    assert_eq!(
        src.metadata.file_type,
        namespace
            .stat(Path::new("/sandbox/work/src"))
            .await
            .unwrap()
            .file_type
    );
    assert_eq!(src.metadata.size, 0);

    let mounted = namespace
        .read_dir(Path::new("/sandbox/work/src"))
        .await
        .unwrap();
    let file = mounted
        .iter()
        .find(|entry| entry.name == "file.txt")
        .unwrap();
    assert_eq!(file.metadata.size, 5);
    assert_eq!(
        file.metadata.size,
        namespace
            .stat(Path::new("/sandbox/work/src/file.txt"))
            .await
            .unwrap()
            .size
    );
}

#[tokio::test]
async fn namespace_rebases_source_root_and_keeps_nested_writable_override() {
    let source = source_with("/repo/src/lib.rs", b"readonly").await;
    source
        .mkdir(Path::new("/repo/src/generated"), false)
        .await
        .unwrap();
    source
        .write_file(Path::new("/repo/private.txt"), b"private")
        .await
        .unwrap();
    let generated = Arc::new(InMemoryFs::new());

    let namespace = NamespaceFs::builder()
        .mount_readonly_from("/src", source.clone(), "/repo/src")
        .unwrap()
        .mount_readwrite("/src/generated", generated.clone())
        .unwrap()
        .build();

    assert_eq!(
        namespace.read_file(Path::new("/src/lib.rs")).await.unwrap(),
        b"readonly"
    );
    assert!(!namespace.exists(Path::new("/private.txt")).await.unwrap());
    assert!(
        namespace
            .write_file(Path::new("/src/lib.rs"), b"blocked")
            .await
            .is_err()
    );
    namespace
        .write_file(Path::new("/src/generated/output.rs"), b"generated")
        .await
        .unwrap();
    assert_eq!(
        generated.read_file(Path::new("/output.rs")).await.unwrap(),
        b"generated"
    );
}

#[tokio::test]
async fn tm_esc_031_namespace_readonly_mount_denies_every_mutation_without_bypass() {
    let source = source_with("/file.txt", b"original").await;
    let namespace = NamespaceFs::builder()
        .mount_readonly("/ro", source.clone())
        .unwrap()
        .mount_readwrite("/rw", Arc::new(InMemoryFs::new()))
        .unwrap()
        .build();

    let operations = [
        namespace
            .write_file(Path::new("/ro/file.txt"), b"changed")
            .await,
        namespace
            .append_file(Path::new("/ro/file.txt"), b"changed")
            .await,
        namespace.remove(Path::new("/ro/file.txt"), false).await,
        namespace.chmod(Path::new("/ro/file.txt"), 0o777).await,
        namespace
            .rename(Path::new("/ro/file.txt"), Path::new("/rw/file.txt"))
            .await,
        namespace.mkdir(Path::new("/ro/new"), false).await,
        namespace
            .copy(Path::new("/rw/missing"), Path::new("/ro/new"))
            .await,
        namespace
            .symlink(Path::new("/target"), Path::new("/ro/link"))
            .await,
        namespace
            .set_modified_time(Path::new("/ro/file.txt"), SystemTime::now())
            .await,
        namespace.mkfifo(Path::new("/ro/fifo"), 0o644).await,
    ];
    for result in operations {
        let Error::Io(error) = result.unwrap_err() else {
            panic!("expected I/O permission error");
        };
        assert_eq!(error.kind(), ErrorKind::PermissionDenied);
    }
    assert_eq!(
        source.read_file(Path::new("/file.txt")).await.unwrap(),
        b"original"
    );
}

#[tokio::test]
async fn tm_esc_031_namespace_normalizes_without_source_or_nested_mount_escape() {
    let source = source_with("/allowed/file.txt", b"allowed").await;
    source
        .write_file(Path::new("/secret.txt"), b"secret")
        .await
        .unwrap();
    let nested = source_with("/nested.txt", b"nested").await;
    let namespace = NamespaceFs::builder()
        .mount_readwrite_from("/visible", source, "/allowed")
        .unwrap()
        .mount_readwrite("/visible/nested", nested)
        .unwrap()
        .build();

    assert_eq!(
        namespace
            .read_file(Path::new("/visible/./file.txt"))
            .await
            .unwrap(),
        b"allowed"
    );
    assert!(
        !namespace
            .exists(Path::new("/visible/../secret.txt"))
            .await
            .unwrap()
    );
    assert!(
        !namespace
            .exists(Path::new("/visible/nested/../nested.txt"))
            .await
            .unwrap()
    );
    assert_eq!(
        namespace
            .read_file(Path::new("/visible/other/../nested/nested.txt"))
            .await
            .unwrap(),
        b"nested"
    );
}

#[tokio::test]
async fn namespace_cross_mount_copy_is_supported_but_rename_is_typed_non_atomic_error() {
    let source = source_with("/file.txt", b"copy me").await;
    let destination = Arc::new(InMemoryFs::new());
    let namespace = NamespaceFs::builder()
        .mount_readonly("/input", source.clone())
        .unwrap()
        .mount_readwrite("/output", destination.clone())
        .unwrap()
        .build();

    namespace
        .copy(Path::new("/input/file.txt"), Path::new("/output/file.txt"))
        .await
        .unwrap();
    assert_eq!(
        destination.read_file(Path::new("/file.txt")).await.unwrap(),
        b"copy me"
    );

    let Error::Io(error) = namespace
        .rename(Path::new("/output/file.txt"), Path::new("/input/moved.txt"))
        .await
        .unwrap_err()
    else {
        panic!("expected typed cross-device error");
    };
    assert_eq!(error.kind(), ErrorKind::PermissionDenied);

    let other = Arc::new(InMemoryFs::new());
    let namespace = NamespaceFs::builder()
        .mount_readwrite("/one", destination)
        .unwrap()
        .mount_readwrite("/two", other)
        .unwrap()
        .build();
    let Error::Io(error) = namespace
        .rename(Path::new("/one/file.txt"), Path::new("/two/file.txt"))
        .await
        .unwrap_err()
    else {
        panic!("expected typed cross-device error");
    };
    assert_eq!(error.kind(), ErrorKind::CrossesDevices);
}

#[tokio::test]
async fn namespace_uses_readonly_wrapper_semantics() {
    let source: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
    let readonly: Arc<dyn FileSystem> = Arc::new(ReadOnlyFs::new(source));
    let namespace = NamespaceFs::builder()
        .mount("/wrapped", readonly, NamespaceAccess::ReadWrite)
        .unwrap()
        .build();

    let Error::Io(error) = namespace
        .write_file(Path::new("/wrapped/file.txt"), b"blocked")
        .await
        .unwrap_err()
    else {
        panic!("expected I/O error");
    };
    assert_eq!(error.kind(), ErrorKind::PermissionDenied);
}

#[tokio::test]
async fn tm_esc_031_namespace_rejects_invalid_paths_and_protects_namespace_nodes() {
    let source = source_with("/nested/file.txt", b"data").await;
    assert!(
        NamespaceFs::builder()
            .mount_readwrite("relative", source.clone())
            .is_err()
    );
    assert!(
        NamespaceFs::builder()
            .mount_readwrite_from("/valid", source.clone(), "relative")
            .is_err()
    );

    let namespace = NamespaceFs::builder()
        .mount_readwrite("/parent", source.clone())
        .unwrap()
        .mount_readwrite("/parent/nested/mount", Arc::new(InMemoryFs::new()))
        .unwrap()
        .build();

    for path in ["/parent", "/parent/nested", "/parent/nested/mount"] {
        let Error::Io(error) = namespace.remove(Path::new(path), true).await.unwrap_err() else {
            panic!("expected namespace-node protection");
        };
        assert_eq!(error.kind(), ErrorKind::PermissionDenied, "{path}");
    }
    assert!(source.exists(Path::new("/nested/file.txt")).await.unwrap());
}

#[tokio::test]
async fn namespace_cross_mount_copy_preserves_symlinks() {
    let source = Arc::new(InMemoryFs::new());
    source
        .symlink(Path::new("/target"), Path::new("/link"))
        .await
        .unwrap();
    let destination = Arc::new(InMemoryFs::new());
    let namespace = NamespaceFs::builder()
        .mount_readonly("/source", source)
        .unwrap()
        .mount_readwrite("/destination", destination.clone())
        .unwrap()
        .build();

    namespace
        .copy(Path::new("/source/link"), Path::new("/destination/link"))
        .await
        .unwrap();
    assert_eq!(
        destination.read_link(Path::new("/link")).await.unwrap(),
        Path::new("/target")
    );
}

#[tokio::test]
async fn namespace_same_mount_copy_and_rename_delegate_atomically() {
    let source = source_with("/original.txt", b"data").await;
    let namespace = NamespaceFs::builder()
        .mount_readwrite("/work", source.clone())
        .unwrap()
        .build();

    namespace
        .copy(
            Path::new("/work/original.txt"),
            Path::new("/work/copied.txt"),
        )
        .await
        .unwrap();
    namespace
        .rename(
            Path::new("/work/copied.txt"),
            Path::new("/work/renamed.txt"),
        )
        .await
        .unwrap();

    assert!(!source.exists(Path::new("/copied.txt")).await.unwrap());
    assert_eq!(
        source.read_file(Path::new("/renamed.txt")).await.unwrap(),
        b"data"
    );
}

#[tokio::test]
async fn namespace_cross_mount_directory_copy_is_typed_unsupported_error() {
    let source = Arc::new(InMemoryFs::new());
    source.mkdir(Path::new("/directory"), false).await.unwrap();
    let namespace = NamespaceFs::builder()
        .mount_readonly("/source", source)
        .unwrap()
        .mount_readwrite("/destination", Arc::new(InMemoryFs::new()))
        .unwrap()
        .build();

    let Error::Io(error) = namespace
        .copy(
            Path::new("/source/directory"),
            Path::new("/destination/directory"),
        )
        .await
        .unwrap_err()
    else {
        panic!("expected typed unsupported error");
    };
    assert_eq!(error.kind(), ErrorKind::Unsupported);
}
