"""Read-only corpus mounts (issue #2387).

A FileSystem mounted with read_only=True is a read-only projection: reads
work, mutations and chmod fail, /tmp stays writable, and the protection
survives reset(). The default stays writable.
"""

import pytest

from bashkit import Bash, BashTool, FileSystem


def make_corpus():
    fs = FileSystem()
    fs.write_file("/report.txt", b"revenue 40,200,000\n")
    return fs


def test_bash_read_only_mount_reads():
    bash = Bash()
    bash.mount("/corpus", make_corpus(), read_only=True)
    r = bash.execute_sync("cat /corpus/report.txt")
    assert r.exit_code == 0
    assert "40,200,000" in r.stdout


def test_bash_read_only_mount_blocks_overwrite():
    bash = Bash()
    bash.mount("/corpus", make_corpus(), read_only=True)
    r = bash.execute_sync("echo 'revenue 999' > /corpus/report.txt; echo rc=$?")
    assert "rc=1" in r.stdout
    assert bash.execute_sync("cat /corpus/report.txt").stdout == "revenue 40,200,000\n"


def test_bash_read_only_mount_blocks_mkdir_and_chmod():
    bash = Bash()
    bash.mount("/corpus", make_corpus(), read_only=True)
    assert bash.execute_sync("mkdir /corpus/newdir").exit_code != 0
    with pytest.raises(Exception):
        bash.chmod("/corpus/report.txt", 0o600)


def test_bash_read_only_mount_keeps_tmp_writable():
    bash = Bash()
    bash.mount("/corpus", make_corpus(), read_only=True)
    r = bash.execute_sync("echo work > /tmp/work.txt && cat /tmp/work.txt")
    assert r.exit_code == 0
    assert "work" in r.stdout


def test_bash_read_only_mount_survives_reset():
    bash = Bash()
    bash.mount("/corpus", make_corpus(), read_only=True)
    bash.reset()
    assert bash.execute_sync("cat /corpus/report.txt").stdout == "revenue 40,200,000\n"
    r = bash.execute_sync("echo 'revenue 999' > /corpus/report.txt; echo rc=$?")
    assert "rc=1" in r.stdout


def test_bash_default_mount_stays_writable():
    bash = Bash()
    bash.mount("/corpus", make_corpus())
    r = bash.execute_sync("echo 'revenue 999' > /corpus/report.txt && cat /corpus/report.txt")
    assert r.exit_code == 0
    assert "revenue 999" in r.stdout


def test_tool_read_only_mount():
    tool = BashTool()
    tool.mount("/corpus", make_corpus(), read_only=True)
    assert tool.execute_sync("cat /corpus/report.txt").stdout == "revenue 40,200,000\n"
    r = tool.execute_sync("echo 'revenue 999' > /corpus/report.txt; echo rc=$?")
    assert "rc=1" in r.stdout
    tool.reset()
    r = tool.execute_sync("echo 'revenue 999' > /corpus/report.txt; echo rc=$?")
    assert "rc=1" in r.stdout
