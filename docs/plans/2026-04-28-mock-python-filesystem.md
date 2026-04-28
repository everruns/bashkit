# Capsule Interop Test Suite — Revised Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Comprehensive test coverage for PR #1353's Python capsule interop (`to_capsule()`/`from_capsule()`), plus a MockFileSystem utility for convenient test setup. The mock is a means to an end — the deliverable is the interop tests.

**Architecture:** MockFileSystem stays as a lightweight test utility (already written). Delete the mock-focused test files (`test_mock_filesystem.py`, `test_mock_vs_real.py`) — testing a test utility is waste. Replace with a single focused `test_capsule_interop.py` that thoroughly exercises the capsule API surface through real bash execution. The playground script stays as a hands-on demo.

**Tech Stack:** Python 3.9+, pytest, bashkit native bindings (PR branch)

---

## What exists now (from previous iteration)

| File | Status | Action |
|------|--------|--------|
| `tests/mock_filesystem.py` | Keep | Test utility — no changes needed |
| `tests/test_mock_filesystem.py` | **Delete** | Testing the mock is waste |
| `tests/test_mock_vs_real.py` | **Delete** | Testing the mock is waste |
| `tests/test_capsule_interop.py` | **Rewrite** | Current version is thin; needs depth |
| `tests/playground_capsule.py` | Keep | Interactive demo — no changes needed |

All paths relative to `crates/bashkit-python/`.

---

### Task 1: Delete mock-focused test files

**Files:**
- Delete: `crates/bashkit-python/tests/test_mock_filesystem.py`
- Delete: `crates/bashkit-python/tests/test_mock_vs_real.py`

**Step 1: Delete the files**

```bash
rm crates/bashkit-python/tests/test_mock_filesystem.py
rm crates/bashkit-python/tests/test_mock_vs_real.py
```

**Step 2: Verify remaining tests still pass**

Run: `/Users/marko/bashkit/.venv/bin/python -m pytest crates/bashkit-python/tests/test_capsule_interop.py -v`
Expected: PASS (6 tests)

**Step 3: Commit**

```bash
git add -u crates/bashkit-python/tests/
git commit -m "test: remove mock-focused tests — mock is utility, not deliverable"
```

---

### Task 2: Rewrite test_capsule_interop.py with comprehensive coverage

**Files:**
- Rewrite: `crates/bashkit-python/tests/test_capsule_interop.py`

The existing PR already has one basic roundtrip test in `_bashkit_categories.py::test_filesystem_capsule_roundtrip_mounts_into_bash`. Our test file should cover the **broader surface area** without duplicating that basic case:

- VFS operations through capsule-imported FS (every FileSystem method)
- Capsule semantics (multiple imports from same capsule, capsule after source mutation)
- Mounting behavior (multi-mount, unmount, nested paths)
- Error cases (invalid capsule, wrong type)
- Bash command execution against mounted capsule FS (grep, find, wc, pipes)

**Step 1: Write the rewritten test file**

```python
# crates/bashkit-python/tests/test_capsule_interop.py
"""Capsule interop tests for FileSystem (PR #1353).

Tests the to_capsule()/from_capsule() API surface: VFS operations through
capsule-imported handles, mount behavior, capsule semantics, error cases,
and bash command execution against mounted capsule filesystems.
"""

import pytest

from bashkit import Bash, FileSystem


# -- VFS operations through capsule-imported FS --------------------------------


class TestCapsuleVfsOps:
    """Every FileSystem method works through a capsule round-trip."""

    @pytest.fixture()
    def imported(self):
        source = FileSystem()
        source.mkdir("/data/sub", recursive=True)
        source.write_file("/data/hello.txt", b"world")
        source.write_file("/data/sub/nested.txt", b"deep")
        source.symlink("/data/hello.txt", "/data/link.txt")
        source.chmod("/data/hello.txt", 0o755)
        return FileSystem.from_capsule(source.to_capsule())

    def test_read_file(self, imported):
        assert imported.read_file("/data/hello.txt") == b"world"

    def test_write_file(self, imported):
        imported.write_file("/data/new.txt", b"created")
        assert imported.read_file("/data/new.txt") == b"created"

    def test_append_file(self, imported):
        imported.append_file("/data/hello.txt", b"!")
        assert imported.read_file("/data/hello.txt") == b"world!"

    def test_mkdir(self, imported):
        imported.mkdir("/data/newdir")
        assert imported.exists("/data/newdir")
        assert imported.stat("/data/newdir")["file_type"] == "directory"

    def test_remove_file(self, imported):
        imported.remove("/data/sub/nested.txt")
        assert not imported.exists("/data/sub/nested.txt")

    def test_remove_dir_recursive(self, imported):
        imported.remove("/data/sub", recursive=True)
        assert not imported.exists("/data/sub")

    def test_exists(self, imported):
        assert imported.exists("/data/hello.txt")
        assert not imported.exists("/data/nope.txt")

    def test_stat(self, imported):
        s = imported.stat("/data/hello.txt")
        assert s["file_type"] == "file"
        assert s["size"] == 5
        assert s["mode"] == 0o755

    def test_read_dir(self, imported):
        names = sorted(e["name"] for e in imported.read_dir("/data"))
        assert "hello.txt" in names
        assert "sub" in names
        assert "link.txt" in names

    def test_symlink_and_read_link(self, imported):
        assert imported.read_link("/data/link.txt") == "/data/hello.txt"

    def test_chmod(self, imported):
        imported.chmod("/data/hello.txt", 0o600)
        assert imported.stat("/data/hello.txt")["mode"] == 0o600

    def test_rename(self, imported):
        imported.rename("/data/hello.txt", "/data/renamed.txt")
        assert imported.read_file("/data/renamed.txt") == b"world"
        assert not imported.exists("/data/hello.txt")

    def test_copy(self, imported):
        imported.copy("/data/hello.txt", "/data/copied.txt")
        assert imported.read_file("/data/copied.txt") == b"world"
        assert imported.read_file("/data/hello.txt") == b"world"


# -- Capsule semantics ---------------------------------------------------------


class TestCapsuleSemantics:
    """Capsule lifecycle: multiple imports, mutation visibility, identity."""

    def test_multiple_imports_share_state(self):
        source = FileSystem()
        source.write_file("/f.txt", b"original")
        capsule = source.to_capsule()

        a = FileSystem.from_capsule(capsule)
        b = FileSystem.from_capsule(capsule)

        a.write_file("/f.txt", b"mutated")
        assert b.read_file("/f.txt") == b"mutated"

    def test_source_mutation_visible_through_capsule(self):
        source = FileSystem()
        source.write_file("/f.txt", b"v1")
        capsule = source.to_capsule()
        imported = FileSystem.from_capsule(capsule)

        source.write_file("/f.txt", b"v2")
        assert imported.read_file("/f.txt") == b"v2"

    def test_imported_mutation_visible_to_source(self):
        source = FileSystem()
        source.write_file("/f.txt", b"original")
        imported = FileSystem.from_capsule(source.to_capsule())

        imported.write_file("/f.txt", b"changed")
        assert source.read_file("/f.txt") == b"changed"

    def test_double_capsule_roundtrip(self):
        fs1 = FileSystem()
        fs1.write_file("/f.txt", b"data")

        fs2 = FileSystem.from_capsule(fs1.to_capsule())
        fs3 = FileSystem.from_capsule(fs2.to_capsule())

        assert fs3.read_file("/f.txt") == b"data"


# -- Mount behavior ------------------------------------------------------------


class TestMountBehavior:
    """Mounting capsule-imported FS into Bash: multi-mount, unmount, paths."""

    def test_mount_and_execute(self):
        source = FileSystem()
        source.write_file("/greeting.txt", b"hello\n")

        bash = Bash()
        bash.mount("/mnt", FileSystem.from_capsule(source.to_capsule()))

        result = bash.execute_sync("cat /mnt/greeting.txt")
        assert result.exit_code == 0
        assert result.stdout == "hello\n"

    def test_multiple_mounts_isolated(self):
        fs1 = FileSystem()
        fs1.write_file("/data.txt", b"from fs1\n")

        fs2 = FileSystem()
        fs2.write_file("/data.txt", b"from fs2\n")

        bash = Bash()
        bash.mount("/mnt/one", FileSystem.from_capsule(fs1.to_capsule()))
        bash.mount("/mnt/two", FileSystem.from_capsule(fs2.to_capsule()))

        r1 = bash.execute_sync("cat /mnt/one/data.txt")
        r2 = bash.execute_sync("cat /mnt/two/data.txt")
        assert r1.stdout == "from fs1\n"
        assert r2.stdout == "from fs2\n"

    def test_unmount_removes_access(self):
        source = FileSystem()
        source.write_file("/f.txt", b"data\n")

        bash = Bash()
        bash.mount("/mnt", FileSystem.from_capsule(source.to_capsule()))
        assert bash.execute_sync("cat /mnt/f.txt").exit_code == 0

        bash.unmount("/mnt")
        result = bash.execute_sync("test -f /mnt/f.txt && echo yes || echo no")
        assert result.stdout.strip() == "no"

    def test_mount_deep_directory_structure(self):
        source = FileSystem()
        source.mkdir("/a/b/c/d", recursive=True)
        source.write_file("/a/b/c/d/deep.txt", b"found\n")

        bash = Bash()
        bash.mount("/workspace", FileSystem.from_capsule(source.to_capsule()))

        result = bash.execute_sync("cat /workspace/a/b/c/d/deep.txt")
        assert result.exit_code == 0
        assert result.stdout == "found\n"

    def test_write_through_mounted_capsule(self):
        source = FileSystem()
        source.mkdir("/data")

        bash = Bash()
        bash.mount("/mnt", FileSystem.from_capsule(source.to_capsule()))
        bash.execute_sync("echo 'written by bash' > /mnt/data/output.txt")

        assert source.read_file("/data/output.txt") == b"written by bash\n"


# -- Error cases ---------------------------------------------------------------


class TestCapsuleErrors:
    """Invalid capsule usage produces clear errors."""

    def test_from_capsule_wrong_type_raises(self):
        with pytest.raises((TypeError, RuntimeError)):
            FileSystem.from_capsule("not a capsule")

    def test_from_capsule_none_raises(self):
        with pytest.raises((TypeError, RuntimeError)):
            FileSystem.from_capsule(None)


# -- Bash commands against capsule FS ------------------------------------------


class TestCapsuleBashCommands:
    """Real bash builtins and pipelines against capsule-mounted filesystems."""

    @pytest.fixture()
    def workspace(self):
        source = FileSystem()
        source.mkdir("/repo/src", recursive=True)
        source.mkdir("/repo/docs")
        source.write_file("/repo/src/lib.py", b"def hello():\n    return 'hello'\n")
        source.write_file("/repo/src/main.py", b"from lib import hello\nprint(hello())\n")
        source.write_file("/repo/docs/guide.md", b"# Guide\n\nSee src/ for code.\n")
        source.write_file("/repo/README.md", b"# Project\n")
        bash = Bash()
        bash.mount("/ws", FileSystem.from_capsule(source.to_capsule()))
        return bash

    def test_find(self, workspace):
        result = workspace.execute_sync("find /ws/repo -name '*.py' | sort")
        assert result.exit_code == 0
        lines = result.stdout.strip().split("\n")
        assert lines == ["/ws/repo/src/lib.py", "/ws/repo/src/main.py"]

    def test_grep_recursive(self, workspace):
        result = workspace.execute_sync("grep -rl 'hello' /ws/repo/src | sort")
        assert result.exit_code == 0
        lines = result.stdout.strip().split("\n")
        assert lines == ["/ws/repo/src/lib.py", "/ws/repo/src/main.py"]

    def test_wc(self, workspace):
        result = workspace.execute_sync("wc -l /ws/repo/src/lib.py")
        assert result.exit_code == 0
        assert "2" in result.stdout

    def test_cat_pipe_grep(self, workspace):
        result = workspace.execute_sync("cat /ws/repo/README.md | grep Project")
        assert result.exit_code == 0
        assert "Project" in result.stdout

    def test_ls(self, workspace):
        result = workspace.execute_sync("ls /ws/repo | sort")
        assert result.exit_code == 0
        names = result.stdout.strip().split("\n")
        assert names == ["README.md", "docs", "src"]

    def test_head(self, workspace):
        result = workspace.execute_sync("head -1 /ws/repo/docs/guide.md")
        assert result.exit_code == 0
        assert result.stdout.strip() == "# Guide"
```

**Step 2: Run tests to verify all pass**

Run: `/Users/marko/bashkit/.venv/bin/python -m pytest crates/bashkit-python/tests/test_capsule_interop.py -v`
Expected: ALL PASS

**Step 3: Commit**

```bash
git add crates/bashkit-python/tests/test_capsule_interop.py
git commit -m "test: rewrite capsule interop tests with full API coverage"
```

---

### Task 3: Run full verification

**Step 1: Run all new + existing tests together**

Run: `/Users/marko/bashkit/.venv/bin/python -m pytest crates/bashkit-python/tests/test_capsule_interop.py crates/bashkit-python/tests/test_vfs.py -v`
Expected: ALL PASS, no regressions

**Step 2: Ruff lint**

Run: `ruff check crates/bashkit-python/tests/test_capsule_interop.py crates/bashkit-python/tests/mock_filesystem.py crates/bashkit-python/tests/playground_capsule.py`
Expected: All checks passed

**Step 3: Run playground**

Run: `/Users/marko/bashkit/.venv/bin/python crates/bashkit-python/tests/playground_capsule.py`
Expected: All 5 sections print, no errors

---

## Final File Summary

| File | Action | Purpose |
|------|--------|---------|
| `tests/mock_filesystem.py` | Keep | Test utility for convenient FS setup |
| `tests/test_capsule_interop.py` | Rewrite | Comprehensive capsule interop test suite (30+ tests) |
| `tests/playground_capsule.py` | Keep | Interactive demo script |
| `tests/test_mock_filesystem.py` | Delete | Was testing the utility, not the feature |
| `tests/test_mock_vs_real.py` | Delete | Was testing the utility, not the feature |

## Test Organization

```
TestCapsuleVfsOps        — every FileSystem method through capsule roundtrip
TestCapsuleSemantics     — mutation visibility, multiple imports, double roundtrip
TestMountBehavior        — mount/unmount, multi-mount, deep paths, write-through
TestCapsuleErrors        — invalid capsule types
TestCapsuleBashCommands  — grep, find, wc, cat|pipe, ls, head against mounted FS
```
