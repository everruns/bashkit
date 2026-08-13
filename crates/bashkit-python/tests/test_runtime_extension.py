"""Runtime host mutations: set_env plus mounts applied after construction,
and their survival across reset() (issue #2291).

The motivating shape is an "extension": a reusable function that mounts a
filesystem, sets env, and registers a builtin on an existing instance. All
three parts must behave the same way across reset(), or the extension is
half-installed after a rebuild.
"""

from bashkit import Bash, BashTool, FileSystem


def skill_filesystem() -> FileSystem:
    fs = FileSystem()
    fs.write_file("/SKILL.md", b"# my-skill\n")
    return fs


# ---------------------------------------------------------------------------
# set_env — live application
# ---------------------------------------------------------------------------


def test_set_env_is_visible_to_scripts():
    bash = Bash()
    bash.set_env("SKILL_PATH", "/skills/my-skill")
    assert bash.execute_sync("echo $SKILL_PATH").stdout == "/skills/my-skill\n"


def test_set_env_is_exported():
    bash = Bash()
    bash.set_env("SKILL_PATH", "/skills/my-skill")
    result = bash.execute_sync("env | grep '^SKILL_PATH='")
    assert result.stdout == "SKILL_PATH=/skills/my-skill\n"


def test_set_env_preserves_existing_shell_state():
    bash = Bash()
    bash.execute_sync("export EXISTING=kept")
    bash.set_env("ADDED", "new")
    assert bash.execute_sync("echo $EXISTING $ADDED").stdout == "kept new\n"


def test_set_env_overrides_constructor_env():
    bash = Bash(env={"SKILL_PATH": "/from-options"})
    bash.set_env("SKILL_PATH", "/from-set-env")
    assert bash.execute_sync("echo $SKILL_PATH").stdout == "/from-set-env\n"


def test_tool_set_env_is_visible_to_scripts():
    tool = BashTool()
    tool.set_env("SKILL_PATH", "/skills/my-skill")
    assert tool.execute_sync("echo $SKILL_PATH").stdout == "/skills/my-skill\n"


# ---------------------------------------------------------------------------
# reset() replay — env
# ---------------------------------------------------------------------------


def test_reset_preserves_set_env():
    bash = Bash()
    bash.set_env("SKILL_PATH", "/skills/my-skill")
    bash.reset()
    assert bash.execute_sync("echo $SKILL_PATH").stdout == "/skills/my-skill\n"


def test_reset_keeps_last_set_env_value():
    bash = Bash()
    bash.set_env("SKILL_PATH", "/first")
    bash.set_env("SKILL_PATH", "/second")
    bash.reset()
    assert bash.execute_sync("echo $SKILL_PATH").stdout == "/second\n"


def test_reset_discards_script_set_env():
    bash = Bash()
    bash.execute_sync("export SCRIPT_ONLY=transient")
    bash.reset()
    assert bash.execute_sync("echo [$SCRIPT_ONLY]").stdout == "[]\n"


def test_reset_keeps_set_env_override_of_constructor_env():
    bash = Bash(env={"SKILL_PATH": "/from-options"})
    bash.set_env("SKILL_PATH", "/from-set-env")
    bash.reset()
    assert bash.execute_sync("echo $SKILL_PATH").stdout == "/from-set-env\n"


def test_tool_reset_preserves_set_env():
    tool = BashTool()
    tool.set_env("SKILL_PATH", "/skills/my-skill")
    tool.reset()
    assert tool.execute_sync("echo $SKILL_PATH").stdout == "/skills/my-skill\n"


# ---------------------------------------------------------------------------
# reset() replay — mounts
# ---------------------------------------------------------------------------


def test_reset_preserves_runtime_mount():
    bash = Bash()
    bash.mount("/skills/my-skill", skill_filesystem())
    bash.reset()
    result = bash.execute_sync("cat /skills/my-skill/SKILL.md")
    assert result.stdout == "# my-skill\n"


def test_unmount_retracts_replay():
    bash = Bash()
    bash.mount("/skills/my-skill", skill_filesystem())
    bash.unmount("/skills/my-skill")
    bash.reset()
    assert bash.execute_sync("cat /skills/my-skill/SKILL.md 2>&1").exit_code != 0


def test_tool_reset_preserves_runtime_mount():
    tool = BashTool()
    tool.mount("/skills/my-skill", skill_filesystem())
    tool.reset()
    assert tool.execute_sync("cat /skills/my-skill/SKILL.md").stdout == "# my-skill\n"


# ---------------------------------------------------------------------------
# The whole extension shape, end to end
# ---------------------------------------------------------------------------


def test_mount_env_builtin_bundle_survives_reset():
    skill_path = "/skills/my-skill"
    data = skill_filesystem()

    def install_skill(bash: Bash) -> None:
        bash.mount(skill_path, data)
        bash.set_env("SKILL_PATH", skill_path)
        bash.add_builtin("my-skill", lambda ctx: ctx.fs.read_file(f"{skill_path}/SKILL.md").decode())

    bash = Bash()
    install_skill(bash)
    bash.reset()

    result = bash.execute_sync('my-skill; cat "$SKILL_PATH/SKILL.md"')
    assert result.exit_code == 0
    assert result.stdout == "# my-skill\n# my-skill\n"
