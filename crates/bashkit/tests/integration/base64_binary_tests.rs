use bashkit::{Bash, FileSystem, InMemoryFs};
use std::path::Path;
use std::sync::Arc;

#[tokio::test]
async fn base64_decode_redirection_preserves_binary_bytes() {
    let fs = Arc::new(InMemoryFs::new());
    let mut bash = Bash::builder().fs(fs.clone()).build();

    let result = bash
        .exec("printf 'AAH//kIAfw==' | base64 -d > /decoded.bin")
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "");
    assert_eq!(
        fs.read_file(Path::new("/decoded.bin")).await.unwrap(),
        vec![0x00, 0x01, 0xff, 0xfe, b'B', 0x00, 0x7f]
    );
}

#[tokio::test]
async fn base64_roundtrips_binary_file_through_redirection() {
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(Path::new("/input.bin"), &[0xff, b'\n', b'\n'])
        .await
        .unwrap();
    let mut bash = Bash::builder().fs(fs.clone()).build();

    let result = bash
        .exec("base64 -w 0 /input.bin | base64 -d > /roundtrip.bin")
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0);
    assert_eq!(
        fs.read_file(Path::new("/roundtrip.bin")).await.unwrap(),
        vec![0xff, b'\n', b'\n']
    );
}
