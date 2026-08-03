// Decision: pathname expansion must glob every path component, not only the
// trailing one. Read-only skill mounts (`/skills/<name>/SKILL.md`) are the
// motivating case: `cat /skills/*/SKILL.md` silently failed because the
// non-final `*` was treated as a literal directory name.

use bashkit::Bash;

async fn skills_shell() -> Bash {
    Bash::builder()
        .mount_readonly_text("/skills/pdf/SKILL.md", "pdf skill\n")
        .mount_readonly_text("/skills/xlsx/SKILL.md", "xlsx skill\n")
        .mount_readonly_text("/skills/xlsx/reference/rows.md", "rows\n")
        .build()
}

#[tokio::test]
async fn intermediate_star_expands_to_matching_files() {
    let mut bash = skills_shell().await;

    let result = bash.exec("echo /skills/*/SKILL.md").await.unwrap();

    assert_eq!(
        result.stdout, "/skills/pdf/SKILL.md /skills/xlsx/SKILL.md\n",
        "non-final glob component must expand"
    );
}

#[tokio::test]
async fn intermediate_star_feeds_builtins() {
    let mut bash = skills_shell().await;

    let result = bash.exec("cat /skills/*/SKILL.md").await.unwrap();

    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "pdf skill\nxlsx skill\n");
}

#[tokio::test]
async fn intermediate_star_expands_in_for_loop() {
    let mut bash = skills_shell().await;

    let result = bash
        .exec("for f in /skills/*/SKILL.md; do echo \"got $f\"; done")
        .await
        .unwrap();

    assert_eq!(
        result.stdout,
        "got /skills/pdf/SKILL.md\ngot /skills/xlsx/SKILL.md\n"
    );
}

#[tokio::test]
async fn multiple_intermediate_globs_expand() {
    let mut bash = skills_shell().await;

    let result = bash.exec("echo /skills/*/*/rows.md").await.unwrap();

    assert_eq!(result.stdout, "/skills/xlsx/reference/rows.md\n");
}

#[tokio::test]
async fn intermediate_bracket_and_question_expand() {
    let mut bash = skills_shell().await;

    let bracket = bash.exec("echo /skills/[px]*/SKILL.md").await.unwrap();
    assert_eq!(
        bracket.stdout,
        "/skills/pdf/SKILL.md /skills/xlsx/SKILL.md\n"
    );

    let question = bash.exec("echo /skills/???/SKILL.md").await.unwrap();
    assert_eq!(question.stdout, "/skills/pdf/SKILL.md\n");
}

#[tokio::test]
async fn intermediate_glob_matches_only_directories() {
    let mut bash = Bash::builder()
        .mount_readonly_text("/skills/pdf/SKILL.md", "pdf\n")
        // A regular file sharing the prefix must not be descended into.
        .mount_readonly_text("/skills/notes", "not a dir\n")
        .build();

    let result = bash.exec("echo /skills/*/SKILL.md").await.unwrap();

    assert_eq!(result.stdout, "/skills/pdf/SKILL.md\n");
}

#[tokio::test]
async fn unmatched_intermediate_glob_stays_literal() {
    let mut bash = skills_shell().await;

    let result = bash.exec("echo /skills/*/MISSING.md").await.unwrap();

    assert_eq!(
        result.stdout, "/skills/*/MISSING.md\n",
        "no match must fall back to the literal pattern (nullglob unset)"
    );
}

#[tokio::test]
async fn unmatched_intermediate_glob_honours_nullglob() {
    let mut bash = skills_shell().await;

    let result = bash
        .exec("shopt -s nullglob; echo begin /skills/*/MISSING.md end")
        .await
        .unwrap();

    assert_eq!(result.stdout, "begin end\n");
}

#[tokio::test]
async fn intermediate_glob_skips_dotdirs_unless_dotglob() {
    let mut bash = Bash::builder()
        .mount_readonly_text("/skills/pdf/SKILL.md", "pdf\n")
        .mount_readonly_text("/skills/.hidden/SKILL.md", "hidden\n")
        .build();

    let default = bash.exec("echo /skills/*/SKILL.md").await.unwrap();
    assert_eq!(default.stdout, "/skills/pdf/SKILL.md\n");

    let dotglob = bash
        .exec("shopt -s dotglob; echo /skills/*/SKILL.md")
        .await
        .unwrap();
    assert_eq!(
        dotglob.stdout,
        "/skills/.hidden/SKILL.md /skills/pdf/SKILL.md\n"
    );

    let explicit_dot = bash.exec("echo /skills/.*/SKILL.md").await.unwrap();
    assert_eq!(explicit_dot.stdout, "/skills/.hidden/SKILL.md\n");
}

#[tokio::test]
async fn intermediate_glob_works_for_relative_paths() {
    let mut bash = skills_shell().await;

    let plain = bash.exec("cd /skills && echo */SKILL.md").await.unwrap();
    assert_eq!(plain.stdout, "pdf/SKILL.md xlsx/SKILL.md\n");

    let dot_slash = bash.exec("cd /skills && echo ./*/SKILL.md").await.unwrap();
    assert_eq!(dot_slash.stdout, "./pdf/SKILL.md ./xlsx/SKILL.md\n");

    let parent = bash
        .exec("cd /skills/pdf && echo ../*/SKILL.md")
        .await
        .unwrap();
    assert_eq!(parent.stdout, "../pdf/SKILL.md ../xlsx/SKILL.md\n");
}

#[tokio::test]
async fn quoted_intermediate_glob_is_literal() {
    let mut bash = skills_shell().await;

    let result = bash.exec("echo \"/skills/*/SKILL.md\"").await.unwrap();

    assert_eq!(result.stdout, "/skills/*/SKILL.md\n");
}

// NOTE: backslash-escaped metacharacters (`echo /skills/\*/SKILL.md`) are NOT
// covered here. Real bash keeps them literal; bashkit expands them because the
// backslash is dropped before pathname expansion runs. That predates this change
// and already affects the trailing component (`echo /skills/\*`), so it is
// tracked separately rather than pinned to the wrong behaviour here.

#[tokio::test]
async fn trailing_slash_glob_still_lists_directories() {
    let mut bash = skills_shell().await;

    let result = bash.exec("echo /skills/*/").await.unwrap();

    assert_eq!(result.stdout, "/skills/pdf /skills/xlsx\n");
}

/// Per-component expansion walks the VFS, so it must not become a traversal
/// primitive: `.`/`..` are not directory entries and cannot be matched by a
/// glob, and `..` past the root stays clamped at the root.
#[tokio::test]
async fn glob_components_cannot_match_dot_entries() {
    let mut bash = Bash::builder()
        .mount_readonly_text("/skills/pdf/SKILL.md", "pdf\n")
        .build();

    // Even with dotglob, `*` must not pick up `.` or `..`.
    let dotglob = bash.exec("shopt -s dotglob; echo /skills/*").await.unwrap();
    assert_eq!(dotglob.stdout, "/skills/pdf\n");

    // An explicit `.` pattern has no `.`/`..` entries to match either.
    let explicit = bash.exec("echo /skills/.*").await.unwrap();
    assert_eq!(explicit.stdout, "/skills/.*\n");
}

#[tokio::test]
async fn glob_walk_stays_clamped_at_vfs_root() {
    let mut bash = Bash::builder()
        .mount_readonly_text("/a/b/deep.txt", "deep\n")
        .build();

    let result = bash
        .exec("cat /a/*/../../../../../../etc/passwd")
        .await
        .unwrap();

    assert_ne!(result.exit_code, 0);
    assert!(
        !result.stdout.contains("root:"),
        "`..` past the VFS root must not reach the host filesystem"
    );
}

#[tokio::test]
async fn readonly_mount_glob_expansion_is_not_writable() {
    let mut bash = skills_shell().await;

    let result = bash
        .exec("echo tampered > /skills/*/SKILL.md; cat /skills/pdf/SKILL.md")
        .await
        .unwrap();

    assert_eq!(
        result.stdout, "pdf skill\n",
        "read-only mounts must stay immutable through glob expansion"
    );
}
