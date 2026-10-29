//! Directory-dot operands copy contents; compare observable layout with real bash.
use bashkit::Bash;

async fn check(script: &str, expected: &str) {
    let result = Bash::new().exec(script).await.unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, expected, "{}", result.stderr);

    #[cfg(unix)]
    {
        let directory = tempfile::tempdir().unwrap();
        // BSD cp lacks the GNU long option; compare equivalent recursive mode.
        let oracle_script = script.replace("cp --recursive ", "cp -R ");
        let oracle = std::process::Command::new("bash")
            .args(["--noprofile", "--norc", "-c", &oracle_script])
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert_eq!(
            oracle.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&oracle.stderr)
        );
        assert_eq!(oracle.stdout, result.stdout.as_bytes());
    }
}

#[tokio::test]
async fn cp_directory_dot_copies_contents_into_existing_directory() {
    for source in ["source/.", "source/./", "source/./."] {
        for flags in ["-r", "-R", "-a", "--recursive"] {
            check(
                &format!(
                    "mkdir -p source/nested destination; printf 'payload\\n' > source/nested/item; \
                     printf 'hidden\\n' > source/.hidden; ln -s nested source/link; \
                     cp {flags} {source} destination; cat destination/nested/item destination/.hidden; \
                     test -L destination/link && readlink destination/link; \
                     test ! -e destination/source && echo no-wrapper"
                ),
                "payload\nhidden\nnested\nno-wrapper\n",
            ).await;
        }
    }
}

#[tokio::test]
async fn cp_directory_dot_can_create_destination() {
    check("mkdir -p source/nested; echo value > source/nested/item; cp -r source/. destination; cat destination/nested/item", "value\n").await;
}

#[tokio::test]
async fn cp_directory_without_dot_retains_basename() {
    for source in ["source/", ".cache", "cache."] {
        let basename = source.trim_end_matches('/');
        check(&format!("mkdir -p {source} destination; echo value > {source}/item; cp -r {source} destination; cat destination/{basename}/item"), "value\n").await;
    }
}

#[tokio::test]
async fn cp_directory_dot_respects_no_clobber() {
    check("mkdir -p source/nested destination; echo replacement > source/item; echo original > destination/item; echo fresh > source/nested/item; cp -rn source/. destination; cat destination/item destination/nested/item", "original\nfresh\n").await;
}

#[tokio::test]
async fn cp_directory_dot_rejects_descendant_and_missing_source() {
    check("mkdir -p source/inside destination; echo keep > source/item; cp -r source/. source/inside 2>/dev/null; echo $?; test ! -e source/inside/item && echo unchanged; cp -r missing/. destination 2>/dev/null; echo $?", "1\nunchanged\n1\n").await;

    // Host cp diagnostics vary (GNU/BSD); compare only status/layout above.
    let result = Bash::new()
        .exec("mkdir -p source/inside; cp -r source/. source/inside")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 1);
    assert_eq!(
        result.stderr,
        "cp: cannot copy a directory, 'source/.', into itself, 'source/inside/.'\n"
    );
}
