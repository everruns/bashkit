//! Tiny ordinary Make substitution fixtures; no expansion chains or large inputs.
use bashkit::{Bash, ExecutionLimits};
use std::path::Path;

const MAKEFILE: &str = "A := $(subst é,🌱,café é)\nB = $(subst aa,z,aaa)\nA += tail\nEMPTY := $(subst ,z,keep)\nall:\n\t@printf '%s|%s|%s\\n' '$(A)' '$(B)' '$(EMPTY)'\n";

#[tokio::test]
async fn tiny_make_substitution_assignments_and_recipes() {
    let mut bash = Bash::builder()
        .cwd("/")
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(32_768))
        .build();
    bash.fs()
        .write_file(Path::new("/Makefile"), MAKEFILE.as_bytes())
        .await
        .unwrap();
    let result = bash.exec("make").await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "stdout={} stderr={}",
        result.stdout, result.stderr
    );
    assert_eq!(result.stdout, "caf🌱 🌱 tail|za|keep\n");
    assert_eq!(result.stderr, "");
    assert_eq!(bash.exec("echo ready").await.unwrap().stdout, "ready\n");
}
