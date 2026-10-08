"""Tests for scripts/oils-spec: the Oils spec-file parser, bash-column
assertions, scoring, and the argv.py shim.

The parser decides the denominator of the headline number, so the Oils rules
it ports (bash overrides win, comment lines dropped, default status 0) each
get a case. No network and no bashkit binary needed.
"""

import importlib.util
import pathlib
import shutil
import subprocess
import tempfile
import unittest

REPO = pathlib.Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("oils_spec_run", REPO / "scripts" / "oils-spec" / "run.py")
run = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(run)

SAMPLE = b"""\
## compare_shells: dash bash mksh

#### plain
echo hi
# dropped comment
echo there
## STDOUT:
hi
there
## END

#### bash override wins
echo x; exit 1
## stdout: y
## status: 0
## OK bash stdout: x
## OK bash status: 1

#### N-I for bash, json stdout
printf '%s' ''
## stdout-json: "a\\u00e9"
## N-I bash/mksh stdout-json: ""

#### stderr only when asserted
echo e >&2
## status: 0
"""


def parse(text):
    with tempfile.TemporaryDirectory() as d:
        path = pathlib.Path(d) / "sample.test.sh"
        path.write_bytes(text)
        return run.parse_spec(path)


def ok(stdout=b"", status=0, stderr=b""):
    return {"stdout": stdout, "stderr": stderr, "status": status}


class ParseTests(unittest.TestCase):
    def setUp(self):
        self.meta, self.cases = parse(SAMPLE)

    def test_metadata_and_bash_column(self):
        self.assertEqual(self.meta["compare_shells"], "dash bash mksh")
        self.assertTrue(run.in_bash_column(self.meta))
        self.assertFalse(run.in_bash_column({"compare_shells": "dash mksh"}))
        self.assertTrue(run.in_bash_column({"compare_shells": "bash-4.4 zsh"}))
        self.assertEqual(len(self.cases), 4)

    def test_comment_lines_dropped_from_code(self):
        self.assertEqual(self.cases[0]["code"], b"echo hi\necho there\n")
        a = run.bash_assertions(self.cases[0])
        self.assertIsNone(run.check(a, ok(b"hi\nthere\n")))
        self.assertEqual(run.check(a, ok(b"hi\n")), "stdout")

    def test_bash_override_wins(self):
        a = run.bash_assertions(self.cases[1])
        self.assertIsNone(run.check(a, ok(b"x\n", 1)))
        self.assertEqual(run.check(a, ok(b"y\n", 0)), "stdout")
        self.assertEqual(run.bash_qualifier(self.cases[1]), "OK")

    def test_shared_override_and_json(self):
        a = run.bash_assertions(self.cases[2])
        self.assertIsNone(run.check(a, ok(b"")))
        self.assertEqual(run.check(a, ok("aé".encode())), "stdout")
        self.assertEqual(run.bash_qualifier(self.cases[2]), "N-I")

    def test_unasserted_stderr_and_default_status(self):
        a = run.bash_assertions(self.cases[3])
        self.assertIsNone(run.check(a, ok(stderr=b"e\n")))
        self.assertEqual(run.check(a, ok(status=1)), "status")
        self.assertEqual(run.check(a, {**ok(), "timeout": True}), "timeout")

    def test_bad_metadata_line_rejected(self):
        with self.assertRaises(ValueError):
            parse(b"#### x\necho\n##stdout: x\n")


class ClassifyTests(unittest.TestCase):
    def test_classes(self):
        self.assertEqual(run.classify({"code": b"kill -TERM $pid\n"}), "signals")
        self.assertEqual(run.classify({"code": b"sleep 1 &\nwait\n"}), "processes")
        self.assertEqual(run.classify({"code": b"complete -F f cmd\n"}), "interactive")
        self.assertEqual(run.classify({"code": b"echo hi\n"}), "behavior")


@unittest.skipUnless(shutil.which("bash") and shutil.which("od"), "needs bash and od")
class ArgvShimTests(unittest.TestCase):
    def test_python2_repr(self):
        args = ["a b", "it's", 'q"x', "μ", "t\tn\n", "back\\slash", "both'\"", ""]
        out = subprocess.run(["bash", str(run.HELPERS / "argv.py"), *args], capture_output=True, check=True).stdout
        self.assertEqual(
            out,
            b"['a b', \"it's\", 'q\"x', '\\xce\\xbc', 't\\tn\\n', 'back\\\\slash', 'both\\'\"', '']\n",
        )


if __name__ == "__main__":
    unittest.main()
