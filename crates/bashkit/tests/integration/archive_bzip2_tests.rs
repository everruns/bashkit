use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use bashkit::{Bash, ExecOptions, ExecutionLimits, FsLimits, InMemoryFs, StreamData};
use bzip2::Compression;
use bzip2::write::BzEncoder;

fn bzip2(input: &[u8]) -> Vec<u8> {
    let mut encoder = BzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(input).unwrap();
    encoder.finish().unwrap()
}

fn tar_with_entry(name: &str, content: &[u8]) -> Vec<u8> {
    let mut header = [0u8; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    header[100..108].copy_from_slice(b"0000644\0");
    header[108..116].copy_from_slice(b"0000000\0");
    header[116..124].copy_from_slice(b"0000000\0");
    header[124..136].copy_from_slice(format!("{:011o}\0", content.len()).as_bytes());
    header[136..148].copy_from_slice(b"00000000000\0");
    header[148..156].copy_from_slice(b"        ");
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar ");
    header[263..265].copy_from_slice(b" \0");
    let checksum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
    header[148..156].copy_from_slice(format!("{:06o}\0 ", checksum).as_bytes());

    let mut archive = header.to_vec();
    archive.extend_from_slice(content);
    archive.resize(512 + content.len().div_ceil(512) * 512, 0);
    archive.extend_from_slice(&[0; 1024]);
    archive
}

#[tokio::test]
async fn bashkit_tar_bzip2_output_is_readable_by_system_tar() {
    let mut bash = Bash::new();
    let setup = bash
        .exec("printf 'system tar reads this\\n' > payload.txt")
        .await
        .unwrap();
    assert_eq!(setup.exit_code, 0, "{}", setup.stderr);

    let result = bash.exec("tar -cjf - payload.txt").await.unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert!(result.stdout.as_bytes().starts_with(b"BZh"));

    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("bashkit.tar.bz2");
    std::fs::write(&archive, result.stdout.as_bytes()).unwrap();
    let listed = Command::new("tar")
        .args(["-tjf", archive.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        listed.status.success(),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    assert_eq!(listed.stdout, b"payload.txt\n");
}

#[tokio::test]
async fn bashkit_lists_and_extracts_system_tar_bzip2_from_stdin() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("input");
    std::fs::create_dir(&input).unwrap();
    std::fs::write(input.join("payload.txt"), b"created by system tar\n").unwrap();
    let archive = temp.path().join("system.tbz2");
    let created = Command::new("tar")
        .env("COPYFILE_DISABLE", "1")
        .args([
            "--format=ustar",
            "-cjf",
            archive.to_str().unwrap(),
            "-C",
            input.to_str().unwrap(),
            "payload.txt",
        ])
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let bytes = std::fs::read(archive).unwrap();

    let mut bash = Bash::new();
    let listed = bash
        .exec_with_options(
            "tar -tf -",
            ExecOptions::new().stdin(StreamData::from(bytes.clone())),
        )
        .await
        .unwrap();
    assert_eq!(listed.exit_code, 0, "{}", listed.stderr);
    assert_eq!(listed.stdout, "payload.txt\n");

    let extracted = bash
        .exec_with_options(
            "mkdir /out; tar -xjf - -C /out",
            ExecOptions::new().stdin(StreamData::from(bytes)),
        )
        .await
        .unwrap();
    assert_eq!(extracted.exit_code, 0, "{}", extracted.stderr);
    assert_eq!(
        bash.fs()
            .read_file(Path::new("/out/payload.txt"))
            .await
            .unwrap(),
        b"created by system tar\n"
    );
}

#[tokio::test]
async fn standalone_bzip2_family_preserves_binary_pipeline_bytes() {
    let expected = vec![0, 1, 2, 0xff, 0xfe, b'Z'];
    let mut bash = Bash::new();
    let result = bash
        .exec_with_options(
            "bzip2 -c | bzcat",
            ExecOptions::new().stdin(StreamData::from(expected.clone())),
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout.as_bytes(), expected);
}

#[tokio::test]
async fn standalone_bzip2_cross_tool_roundtrips_with_system_bzip2() {
    let payload = b"cross-tool bzip2 fixture\n";
    let mut bash = Bash::new();
    let encoded = bash
        .exec_with_options(
            "bzip2 -c",
            ExecOptions::new().stdin(StreamData::from(payload.to_vec())),
        )
        .await
        .unwrap();
    assert_eq!(encoded.exit_code, 0, "{}", encoded.stderr);

    let mut child = Command::new("bzip2")
        .arg("-dc")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(encoded.stdout.as_bytes())
        .unwrap();
    let decoded = child.wait_with_output().unwrap();
    assert!(decoded.status.success());
    assert_eq!(decoded.stdout, payload);

    let system_encoded = Command::new("bzip2")
        .arg("-c")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child.stdin.take().unwrap().write_all(payload)?;
            child.wait_with_output()
        })
        .unwrap();
    assert!(system_encoded.status.success());
    let decoded = bash
        .exec_with_options(
            "bzcat",
            ExecOptions::new().stdin(StreamData::from(system_encoded.stdout)),
        )
        .await
        .unwrap();
    assert_eq!(decoded.exit_code, 0, "{}", decoded.stderr);
    assert_eq!(decoded.stdout.as_bytes(), payload);
}

#[tokio::test]
async fn bzip2_file_modes_keep_remove_force_and_tbz2_suffix_match_tools() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            "printf payload > data; bzip2 -k data; test -f data; test -f data.bz2; \
             bzcat data.bz2; mv data.bz2 sample.tbz2; bunzip2 -kf sample.tbz2; \
             test -f sample.tbz2; cat sample.tar",
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, "payloadpayload");

    let listed = bash
        .exec("tar -cjf named.tar.bz2 data; tar -tf named.tar.bz2")
        .await
        .unwrap();
    assert_eq!(listed.exit_code, 0, "{}", listed.stderr);
    assert_eq!(listed.stdout, "data\n");
}

#[tokio::test]
/// TM-DOS-007/TM-DOS-102: malformed streams and expansion bombs fail closed.
async fn bunzip2_rejects_truncation_corruption_crc_and_ratio_bombs() {
    let valid = bzip2(b"bounded bzip2 payload");
    let mut corrupt = valid.clone();
    corrupt[10] ^= 0x40;
    let mut bad_crc = valid.clone();
    let crc_index = bad_crc.len() - 5;
    bad_crc[crc_index] ^= 0x01;
    let cases = [
        (&valid[..valid.len() - 4], "truncated"),
        (&corrupt, "corrupt"),
        (&bad_crc, "crc"),
    ];

    for (input, label) in cases {
        let mut bash = Bash::new();
        let result = bash
            .exec_with_options(
                "bunzip2 -c",
                ExecOptions::new().stdin(StreamData::from(input.to_vec())),
            )
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0, "{label} input unexpectedly succeeded");
        assert!(result.stdout.is_empty(), "{label} emitted partial output");
    }

    let bomb = bzip2(&vec![b'x'; 100_000]);
    let mut bash = Bash::new();
    let result = bash
        .exec_with_options(
            "bunzip2 -c",
            ExecOptions::new().stdin(StreamData::from(bomb)),
        )
        .await
        .unwrap();
    assert_ne!(result.exit_code, 0);
    assert!(result.stderr.contains("ratio"), "{}", result.stderr);
}

#[tokio::test]
async fn compressed_tar_keeps_path_and_file_size_guards() {
    let traversal = bzip2(&tar_with_entry("../../escape", b"blocked"));
    let mut bash = Bash::new();
    let result = bash
        .exec_with_options(
            "tar -xjf -",
            ExecOptions::new().stdin(StreamData::from(traversal)),
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 2);
    assert!(result.stderr.contains("path traversal blocked"));
    assert!(!bash.fs().exists(Path::new("/escape")).await.unwrap());

    let limits = FsLimits::new().max_file_size(32).max_total_bytes(1_000);
    let fs = Arc::new(InMemoryFs::with_limits(limits));
    let mut bash = Bash::builder().fs(fs).build();
    let oversized = bzip2(&[b'a'; 33]);
    let result = bash
        .exec_with_options(
            "bunzip2 -c",
            ExecOptions::new().stdin(StreamData::from(oversized)),
        )
        .await
        .unwrap();
    assert_ne!(result.exit_code, 0);
    assert!(result.stderr.contains("limit"), "{}", result.stderr);
}

#[tokio::test]
async fn bzip2_charges_shared_input_and_live_memory_budgets() {
    let compressed = bzip2(b"execution budget payload");
    let limits = ExecutionLimits::new()
        .max_aggregate_input_bytes(1_000)
        .max_live_intermediate_bytes((compressed.len() + 8) as u64);
    let mut bash = Bash::builder().limits(limits).build();
    let result = bash
        .exec_with_options(
            "bunzip2 -c",
            ExecOptions::new().stdin(StreamData::from(compressed)),
        )
        .await;
    assert!(result.is_err(), "budget exhaustion must fail the request");
}
