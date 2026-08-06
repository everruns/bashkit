//! Backend-neutral filesystem security certification helpers.

use bashkit::{Error, FileSystem};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

fn io_kind(error: Error) -> ErrorKind {
    match error {
        Error::Io(error) => error.kind(),
        other => panic!("expected I/O error, got {other}"),
    }
}

pub async fn certify_path_and_data_contract(name: &str, fs: &dyn FileSystem) {
    fs.mkdir(Path::new("/cert"), false).await.unwrap();
    let content = b"prefix\0middle\0suffix";
    fs.write_file(Path::new("/cert/data"), content)
        .await
        .unwrap();
    assert_eq!(
        fs.read_file(Path::new("/../../cert/./data")).await.unwrap(),
        content,
        "{name}"
    );
    fs.symlink(Path::new("data"), Path::new("/cert/link"))
        .await
        .unwrap();
    assert!(
        fs.stat(Path::new("/cert/link"))
            .await
            .unwrap()
            .file_type
            .is_symlink(),
        "{name}"
    );
    assert_eq!(
        io_kind(fs.read_file(Path::new("/cert/link")).await.unwrap_err()),
        ErrorKind::NotFound,
        "{name}"
    );
    assert_eq!(
        fs.read_link(Path::new("/cert/link")).await.unwrap(),
        PathBuf::from("data"),
        "{name}"
    );

    let unsafe_path = Path::new("/cert/bad\0name");
    let errors = [
        fs.read_file(unsafe_path).await.unwrap_err(),
        fs.write_file(unsafe_path, b"x").await.unwrap_err(),
        fs.mkdir(unsafe_path, false).await.unwrap_err(),
        fs.remove(unsafe_path, false).await.unwrap_err(),
        fs.stat(unsafe_path).await.unwrap_err(),
        fs.read_dir(unsafe_path).await.unwrap_err(),
        fs.copy(Path::new("/cert/data"), unsafe_path)
            .await
            .unwrap_err(),
        fs.rename(Path::new("/cert/data"), unsafe_path)
            .await
            .unwrap_err(),
        fs.symlink(Path::new("target"), unsafe_path)
            .await
            .unwrap_err(),
        fs.read_link(unsafe_path).await.unwrap_err(),
        fs.chmod(unsafe_path, 0o600).await.unwrap_err(),
    ];
    for error in errors {
        assert_eq!(io_kind(error), ErrorKind::InvalidInput, "{name}");
    }
}
