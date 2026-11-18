use std::process::Command;
use std::sync::{Arc, Mutex};

use bashkit::{Bash, Builtin, BuiltinContext, ExecOptions, ExecResult, StreamData, async_trait};

#[tokio::test]
async fn binary_base64_head_cat_pipeline_preserves_exact_bytes() {
    let mut bash = Bash::new();
    let result = bash
        .exec("printf 'AAH//kIAfw==' | base64 -d | head -c 7 | cat")
        .await
        .unwrap();

    assert_eq!(
        result.stdout.as_bytes(),
        &[0x00, 0x01, 0xff, 0xfe, b'B', 0x00, 0x7f]
    );
}

#[tokio::test]
async fn mixed_utf8_and_invalid_bytes_survive_stdin_pipeline_and_redirect() {
    let input = StreamData::from(vec![b'h', 0xc3, 0xa9, 0xff, 0x00, b'\n']);
    let mut bash = Bash::new();
    let result = bash
        .exec_with_options(
            "cat > /blob; cat /blob | base64 -w 0",
            ExecOptions::new().stdin(input),
        )
        .await
        .unwrap();

    assert_eq!(result.stdout.text().unwrap(), "aMOp/wAK");
    assert_eq!(
        bash.fs()
            .read_file(std::path::Path::new("/blob"))
            .await
            .unwrap(),
        [b'h', 0xc3, 0xa9, 0xff, 0x00, b'\n']
    );
}

#[tokio::test]
async fn byte_output_limit_and_streaming_callback_split_without_utf8_rewrite() {
    let chunks = Arc::new(Mutex::new(Vec::<u8>::new()));
    let callback_chunks = Arc::clone(&chunks);
    let mut bash = Bash::builder()
        .limits(bashkit::ExecutionLimits::new().max_stdout_bytes(4))
        .build();
    let result = bash
        .exec_with_options(
            "printf '/wABAg==' | base64 -d",
            ExecOptions::new().streaming(Box::new(move |stdout, _stderr| {
                callback_chunks
                    .lock()
                    .unwrap()
                    .extend_from_slice(stdout.as_bytes());
            })),
        )
        .await
        .unwrap();

    assert_eq!(result.stdout.as_bytes(), &[0xff, 0x00, 0x01, 0x02]);
    assert_eq!(*chunks.lock().unwrap(), [0xff, 0x00, 0x01, 0x02]);
}

struct CopyBytes;

#[async_trait]
impl Builtin for CopyBytes {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        Ok(ExecResult::ok(ctx.stdin.cloned().unwrap_or_default()))
    }
}

#[tokio::test]
async fn custom_builtin_receives_byte_native_stdin() {
    let mut bash = Bash::builder()
        .builtin("copy-bytes", Box::new(CopyBytes))
        .build();
    let result = bash
        .exec("printf '/wD+' | base64 -d | copy-bytes")
        .await
        .unwrap();
    assert_eq!(result.stdout.as_bytes(), &[0xff, 0x00, 0xfe]);
}

#[tokio::test]
async fn execution_plan_preserves_binary_stdin() {
    let mut bash = Bash::new();
    let result = bash
        .exec("printf '/wD+' | base64 -d | timeout 1 cat")
        .await
        .unwrap();
    assert_eq!(result.stdout.as_bytes(), &[0xff, 0x00, 0xfe]);
}

#[tokio::test]
async fn c_locale_tr_filters_invalid_bytes_without_replacement_text() {
    let input = StreamData::from(vec![0xff, b'a', 0x80, b'7', 0x00, b'Z']);
    let mut bash = Bash::new();
    let result = bash
        .exec_with_options("LC_ALL=C tr -dc 'a-z0-9'", ExecOptions::new().stdin(input))
        .await
        .unwrap();

    assert_eq!(result.stdout.as_bytes(), b"a7");
}

#[tokio::test]
async fn command_substitution_drops_nuls_at_text_boundary() {
    let mut bash = Bash::new();
    let result = bash
        .exec("x=$(printf 'QQBCAA==' | base64 -d); printf '%s' \"$x\"")
        .await
        .unwrap();

    assert_eq!(result.stdout.as_bytes(), b"AB");
}

#[tokio::test]
async fn binary_pipeline_matches_real_bash_byte_for_byte() {
    let script = "printf 'AAHDqf/+QgB/' | base64 -d | head -c 8 | cat";
    let expected = Command::new("bash").args(["-c", script]).output().unwrap();
    assert!(expected.status.success());

    let mut bash = Bash::new();
    let actual = bash.exec(script).await.unwrap();
    assert_eq!(actual.stdout.as_bytes(), expected.stdout);
    assert_eq!(actual.exit_code, expected.status.code().unwrap());
}

#[tokio::test]
async fn read_consumes_binary_pipeline_lines_by_raw_byte_offset() {
    let mut bash = Bash::new();
    let result = bash
        .exec("printf '/wpB' | base64 -d | { read first; read second; printf '%s' \"$second\"; }")
        .await
        .unwrap();

    assert_eq!(result.stdout.as_bytes(), b"A");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn select_consumes_binary_pipeline_lines_by_raw_byte_offset() {
    let mut bash = Bash::new();
    let result = bash
        .exec_with_options(
            "select choice in ok; do if [ -n \"$choice\" ]; then printf '%s' \"$choice\"; break; fi; done",
            ExecOptions::new().stdin(vec![0xff, b'\n', b'1', b'\n']),
        )
        .await
        .unwrap();

    assert_eq!(
        result.stdout.as_bytes(),
        b"ok",
        "stderr: {}; exit: {}",
        result.stderr,
        result.exit_code
    );
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn iconv_repairs_cp_export_and_writes_real_bytes() {
    // Latin-1 export in, UTF-8 for text tools, then Latin-1 / UTF-16 bytes out.
    let mut bash = Bash::new();
    let result = bash
        .exec(
            "printf 'K\\366ln;1\\n' > /e.csv
             iconv -f latin1 -t utf-8 -o /e.csv /e.csv
             cut -d';' -f1 /e.csv | iconv -f utf-8 -t latin1 | od -An -tx1
             printf 'K\\303\\203\\302\\266ln' | iconv -f utf-8 -t latin1 | iconv -f utf-8 -t utf-16le | od -An -tx1",
        )
        .await
        .unwrap();
    assert_eq!(
        result.stdout.text().unwrap(),
        " 4b f6 6c 6e 0a\n 4b 00 f6 00 6c 00 6e 00\n"
    );
    assert_eq!(
        bash.fs()
            .read_file(std::path::Path::new("/e.csv"))
            .await
            .unwrap(),
        "Köln;1\n".as_bytes()
    );
}

#[tokio::test]
async fn iconv_matches_real_iconv_byte_for_byte() {
    // Skip where the host has no iconv; CI runners ship glibc's.
    if Command::new("iconv").arg("-l").output().is_err() {
        return;
    }
    for script in [
        "printf 'K\\303\\266ln \\342\\202\\254\\n' | iconv -f utf-8 -t utf-16",
        "printf 'K\\303\\266ln\\n' | iconv -f utf-8 -t utf-16be",
        "printf 'K\\303\\266ln\\n' | iconv -f utf-8 -t latin1",
        "printf 'K\\366ln\\n' | iconv -f iso-8859-1 -t utf-8",
        "printf '\\377\\376a\\000\\330\\075\\336\\000' | iconv -f utf-16 -t utf-8",
        // -c exit status differs on purpose (L-ICONV-001); bytes must match.
        "printf 'a\\377b\\n' | iconv -c -f utf-8 -t utf-8; true",
        "printf 'ab\\303' | iconv -f utf-8 -t latin1",
    ] {
        let expected = Command::new("bash").args(["-c", script]).output().unwrap();
        let mut bash = Bash::new();
        let actual = bash.exec(script).await.unwrap();
        assert_eq!(actual.stdout.as_bytes(), expected.stdout, "{script}");
        assert_eq!(
            actual.exit_code,
            expected.status.code().unwrap(),
            "{script}"
        );
    }
}
