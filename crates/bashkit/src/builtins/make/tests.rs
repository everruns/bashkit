use crate::Bash;

async fn run(setup: &str, script: &str) -> (String, String, i32) {
    let mut bash = Bash::new();
    bash.exec(setup).await.unwrap();
    let r = bash.exec(script).await.unwrap();
    (r.stdout.to_string(), r.stderr.to_string(), r.exit_code)
}

#[tokio::test]
async fn builds_then_reports_up_to_date() {
    let mk = "printf 'out.txt: in.txt\\n\\tcp in.txt out.txt\\n' > Makefile; echo hi > in.txt";
    let (o, e, c) = run(mk, "make; make; cat out.txt").await;
    assert_eq!(e, "");
    assert_eq!(c, 0);
    assert_eq!(o, "cp in.txt out.txt\nmake: 'out.txt' is up to date.\nhi\n");
}

#[tokio::test]
async fn recipe_failure_stops_with_location() {
    let (o, e, c) = run("printf 'x:\\n\\tfalse\\n\\techo no\\n' > Makefile", "make").await;
    assert_eq!(o, "false\n");
    assert_eq!(e, "make: *** [Makefile:2: x] Error 1\n");
    assert_eq!(c, 2);
}

#[tokio::test]
async fn missing_rule_and_no_makefile() {
    let (_, e, c) = run("", "make").await;
    assert_eq!(
        e,
        "make: *** No targets specified and no makefile found.  Stop.\n"
    );
    assert_eq!(c, 2);
    let (_, e, c) = run("printf 'a: b\\n\\t@echo a\\n' > Makefile", "make").await;
    assert_eq!(
        e,
        "make: *** No rule to make target 'b', needed by 'a'.  Stop.\n"
    );
    assert_eq!(c, 2);
}

#[tokio::test]
async fn shell_and_wildcard_functions() {
    let setup = "touch a.c b.c; printf 'SRC := $(wildcard *.c)\\nN != echo 3\\nall:\\n\\t@echo $(SRC:.c=.o) $(N) $(shell echo x)\\n' > Makefile";
    let (o, e, c) = run(setup, "make").await;
    assert_eq!((o.as_str(), e.as_str(), c), ("a.o b.o 3 x\n", "", 0));
}

#[tokio::test]
async fn recursive_make_and_dry_run() {
    let setup = "mkdir sub; printf 'all:\\n\\techo sub $(V)\\n' > sub/Makefile; printf 'all:\\n\\t$(MAKE) -C sub V=1\\n' > Makefile";
    let (o, _, c) = run(setup, "make -s").await;
    assert_eq!((o.as_str(), c), ("sub 1\n", 0));
    let (o, _, c) = run(setup, "make -n").await;
    assert_eq!(c, 0);
    assert!(o.contains("echo sub 1"), "{o}");
    assert!(!o.contains("\nsub 1\n"), "{o}");
}

#[tokio::test]
async fn self_recursive_make_is_bounded() {
    let (_, e, c) = run("printf 'all:\\n\\t@$(MAKE)\\n' > Makefile", "make").await;
    assert_ne!(c, 0);
    assert!(e.contains("recursive make depth exceeds"), "{e}");
}
