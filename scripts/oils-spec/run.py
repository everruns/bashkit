#!/usr/bin/env python3
"""Oils spec pass rate: the upstream Oils spec suite, bash column, through bashkit.

Oils (https://github.com/oils-for-unix/oils, Apache-2.0) keeps ~2,800 shell
spec cases with expected output per shell. This harness fetches the suite at a
pinned commit into a cache outside the repo (never vendored), runs every case
whose file compares against bash through bashkit and through real bash, and
publishes one headline number:

    pass rate = cases bashkit passes / cases real bash passes

A case passes for a shell when its status, stdout and any asserted stderr match
the bash column of the case: `## OK bash`, `## N-I bash` and `## BUG bash`
overrides win over the generic `## stdout:` / `## status:` values, exactly as
Oils' own runner (`test/sh_spec.py`) builds assertions for a `bash` label.

Important decisions:
- Real bash runs the Oils way: code on stdin, env `SH`, `TMP`, `REPO_ROOT`,
  `LC_ALL=C.UTF-8`, cwd a fresh temp dir. Its pass count is a sanity check of
  this parser against Oils' published bash numbers, and the denominator.
- bashkit runs `bashkit -c CODE bash` (`$0` is `bash`, as with stdin) with
  stdin closed. The CLI does not import host env, so the same variables are
  exported by a prefix glued onto the first code line, keeping `$LINENO`
  identical. The Oils checkout and the helper shims are mounted read-only
  (`--mount-ro`, needs the CLI `realfs` feature).
- Oils' helpers in `spec/bin` are Python 2; `bin/` here holds bash stand-ins
  with the same output, used by both shells, because bashkit runs no host
  Python and the runner may have no Python 2.
- A failing case is classified (signals, processes, interactive, host) by a
  keyword heuristic so gaps bashkit keeps by design are visible, but every
  case still counts in the rate.
"""

import argparse
import concurrent.futures
import datetime
import json
import os
import pathlib
import platform
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
HELPERS = HERE / "bin"
RESULTS = HERE / "results"

OILS_REPO = "https://github.com/oils-for-unix/oils"
# Oils pushes no release tags to GitHub; pin a master commit (2026-10-06).
OILS_REV = "57d3f0d088c36340bc4b3038208e58ea02acf15c"

# VFS paths inside bashkit for the read-only mounts.
VFS_REPO = "/oils"
VFS_HELPERS = "/oils-spec-bin"


# --- Oils spec file format (port of test/sh_spec.py, bash column only) ------

KEY_VALUE_RE = re.compile(
    rb"""
   [#][#] \s+
   (?: (OK(?:-\d)? | BUG(?:-\d)? | N-I) \s+ ([\w+/]+) \s+ )?
   ([\w\-]+)
   :
   \s* (.*)
""",
    re.VERBOSE,
)
END_MULTILINE_RE = re.compile(rb"[#][#] \s+ END", re.VERBOSE)

CASE_BEGIN, KEY_VALUE, KEY_VALUE_MULTILINE, END_MULTILINE, PLAIN, EOF = range(6)


class Tokenizer:
    """Modal line lexer, same rules as sh_spec.py.

    Comment lines are dropped everywhere, including inside code and STDOUT
    blocks; Oils runs the code that way, so line numbers match it.
    """

    def __init__(self, lines):
        self.lines = lines
        self.pos = 0
        self.cursor = None
        self.next()

    def _classify(self, line, raw):
        if line is None:
            return EOF, b""
        if not raw and not line.strip():
            return None
        if line.startswith(b"####"):
            return CASE_BEGIN, line[4:].strip().decode("utf-8", "replace")
        m = KEY_VALUE_RE.match(line)
        if m:
            qualifier, shells, name, value = m.groups()
            name = name.decode()
            if name in ("stdout", "stderr"):
                value += b"\n"
            kind = KEY_VALUE_MULTILINE if name in ("STDOUT", "STDERR") else KEY_VALUE
            q = qualifier.decode() if qualifier else None
            s = shells.decode() if shells else None
            return kind, (q, s, name, value)
        if END_MULTILINE_RE.match(line):
            return END_MULTILINE, None
        if line.startswith(b"##"):
            raise ValueError(f"invalid ## line {line!r}")
        if line.lstrip().startswith(b"#"):
            return None
        return PLAIN, line

    def next(self, raw=False):
        while True:
            line = self.lines[self.pos] if self.pos < len(self.lines) else None
            self.pos += 1
            tok = self._classify(line, raw)
            if tok is not None:
                self.cursor = tok
                return tok

    def peek(self):
        return self.cursor


def _add_meta(case, qualifier, shells, name, value):
    for shell in shells.split("/"):
        d = case.setdefault("by_shell", {}).setdefault(shell, {})
        d[name] = value
        d["qualifier"] = qualifier


def _parse_key_values(tokens, case):
    while True:
        kind, item = tokens.peek()
        if kind == KEY_VALUE_MULTILINE:
            qualifier, shells, name, _ = item
            lines = []
            while True:
                kind2, item2 = tokens.next(raw=True)
                if kind2 != PLAIN:
                    break
                lines.append(item2)
            value = b"".join(lines)
            name = name.lower()
            if qualifier:
                _add_meta(case, qualifier, shells, name, value)
            else:
                case[name] = value
            if kind2 == END_MULTILINE:
                tokens.next()
        elif kind == KEY_VALUE:
            qualifier, shells, name, value = item
            if qualifier:
                _add_meta(case, qualifier, shells, name, value)
            else:
                case[name] = value
            tokens.next()
        else:
            return


def parse_spec(path):
    """Return (file metadata, cases) for one `*.test.sh` file."""
    lines = path.read_bytes().splitlines(keepends=True)
    tokens = Tokenizer(lines)
    meta = {}
    while True:
        kind, item = tokens.peek()
        if kind != KEY_VALUE:
            break
        meta[item[2]] = item[3].decode().strip()
        tokens.next()
    cases = []
    while True:
        kind, item = tokens.peek()
        if kind == EOF:
            break
        if kind != CASE_BEGIN:
            raise ValueError(f"{path.name}: expected ####, got {item!r}")
        tokens.next()
        case = {"desc": item}
        _parse_key_values(tokens, case)
        if "code" not in case:
            code = []
            if tokens.peek()[0] != PLAIN:
                raise ValueError(f"{path.name}: case {item!r} has no code")
            while tokens.peek()[0] == PLAIN:
                code.append(tokens.peek()[1])
                tokens.next(raw=True)
            case["code"] = b"".join(code)
            _parse_key_values(tokens, case)
        cases.append(case)
    return meta, cases


def _string_assertion(d, key, out, qualifier):
    found = False
    if key in d:
        out.append((key, d[key], qualifier))
        found = True
    if key + "-json" in d:
        value = json.loads(d[key + "-json"].decode("utf-8")).encode("utf-8")
        out.append((key, value, qualifier))
        found = True
    return found


def bash_assertions(case):
    """(key, expected, qualifier) triples for the `bash` column of a case."""
    out = []
    got = {"stdout": False, "stderr": False, "status": False}
    over = case.get("by_shell", {}).get("bash")
    if over:
        q = over["qualifier"]
        got["stdout"] = _string_assertion(over, "stdout", out, q)
        got["stderr"] = _string_assertion(over, "stderr", out, q)
        if "status" in over:
            out.append(("status", int(over["status"]), q))
            got["status"] = True
    if not got["stdout"]:
        _string_assertion(case, "stdout", out, None)
    if not got["stderr"]:
        _string_assertion(case, "stderr", out, None)
    if not got["status"]:
        out.append(("status", int(case.get("status", b"0")), None))
    return out


def check(assertions, actual):
    """None when every assertion holds, else the first failing key."""
    if actual.get("timeout"):
        return "timeout"
    for key, expected, _ in assertions:
        if actual[key] != expected:
            return key
    if b"Traceback (most recent" in actual["stderr"]:
        return "stderr"
    return None


def bash_qualifier(case):
    over = case.get("by_shell", {}).get("bash")
    return over["qualifier"] if over else "PASS"


def in_bash_column(meta):
    shells = meta.get("compare_shells", "").split()
    return any(s == "bash" or s.startswith("bash-") for s in shells)


# --- classification of bashkit failures -------------------------------------

# Heuristic, ordered: first match wins. These are areas bashkit keeps out of
# the sandbox by design (knowledge/operations/limitations.md); a failing case
# that touches one is labeled so the gap list separates them from plain
# behavior differences. They still count in the pass rate.
CLASSES = [
    (
        "signals",
        re.compile(rb"\bkill\b|\bSIG[A-Z]+\b|\btrap\b[^\n]*\b(INT|TERM|USR1|USR2|HUP|QUIT|CHLD|ALRM|PIPE|WINCH)\b"),
    ),
    (
        "processes",
        re.compile(
            rb"\bwait\b|\bjobs\b|\bfg\b|\bbg\b|\bdisown\b|\$!|\$\$|\$PPID|\$BASHPID|/proc/|\bulimit\b|\btimes\b|\bps\b|\bcoproc\b|\bsleep\b[^\n]*&"
        ),
    ),
    (
        "interactive",
        re.compile(rb"\$SH -i\b| -i -c\b|\bPS[0-4]\b|\bbind\b|\bhistory\b|\bfc\b|\bcomplete\b|\bcompgen\b|\bcompopt\b"),
    ),
    (
        "host",
        re.compile(
            rb"/dev/tty|/etc/passwd|~root\b|\bumask\b|\bhostname\b|\$UID\b|\$EUID\b|\bid -|\bchmod\b|\bmkfifo\b"
        ),
    ),
]


def classify(case):
    code = case["code"]
    for name, rx in CLASSES:
        if rx.search(code):
            return name
    return "behavior"


# --- running -----------------------------------------------------------------


def run_proc(argv, stdin_bytes, cwd, env, timeout, executable=None):
    proc = subprocess.Popen(
        argv,
        executable=executable,
        stdin=subprocess.PIPE if stdin_bytes is not None else subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        cwd=cwd,
        env=env,
        start_new_session=True,
    )
    try:
        out, err = proc.communicate(stdin_bytes, timeout=timeout)
        return {"stdout": out, "stderr": err, "status": proc.returncode}
    except subprocess.TimeoutExpired:
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        try:
            proc.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            pass
        return {"stdout": b"", "stderr": b"", "status": -9, "timeout": True}
    except BrokenPipeError:
        out, err = proc.communicate(timeout=timeout)
        return {"stdout": out, "stderr": err, "status": proc.returncode}


def bashkit_prefix(legacy_tmp):
    prefix = f"export SH=bash TMP=/tmp REPO_ROOT={VFS_REPO} LC_ALL=C.UTF-8 PATH={VFS_HELPERS}:$PATH; "
    if legacy_tmp:
        prefix += "mkdir -p _tmp; "
    return prefix.encode()


def run_bashkit(binary, repo, code, timeout, legacy_tmp):
    with tempfile.TemporaryDirectory(prefix="oils-bk-") as work:
        argv = [
            binary,
            "--mount-ro",
            f"{repo}:{VFS_REPO}",
            "--mount-ro",
            f"{HELPERS}:{VFS_HELPERS}",
            "--timeout",
            str(timeout),
            "-c",
            (bashkit_prefix(legacy_tmp) + code).decode("utf-8", "surrogateescape"),
            "bash",
        ]
        env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "LC_ALL": "C.UTF-8"}
        return run_proc(argv, None, work, env, timeout + 5)


def run_bash(bash, repo, code, timeout, legacy_tmp):
    with tempfile.TemporaryDirectory(prefix="oils-sh-") as work:
        if legacy_tmp:
            os.mkdir(os.path.join(work, "_tmp"))
        env = {
            "PATH": f"{HELPERS}:{os.path.dirname(bash)}:{os.environ.get('PATH', '/usr/bin:/bin')}",
            "LC_ALL": "C.UTF-8",
            # A bare name, as Oils passes it: bash then reports errors as
            # "bash: line N:", which is what stderr assertions expect.
            "SH": "bash",
            "TMP": work,
            "REPO_ROOT": str(repo),
        }
        return run_proc(["bash"], code, work, env, timeout, executable=bash)


def fetch_oils(cache):
    src = cache / "oils"
    head = subprocess.run(["git", "-C", str(src), "rev-parse", "HEAD"], capture_output=True, text=True, check=False)
    if head.returncode == 0 and head.stdout.strip() == OILS_REV:
        return src
    if src.exists():
        shutil.rmtree(src)
    src.mkdir(parents=True)
    for argv in (
        ["git", "init", "-q", str(src)],
        ["git", "-C", str(src), "fetch", "-q", "--depth", "1", OILS_REPO, OILS_REV],
        ["git", "-C", str(src), "checkout", "-q", "FETCH_HEAD"],
    ):
        subprocess.run(argv, check=True)
    return src


def supports_mounts(binary):
    probe = subprocess.run(
        [binary, "--mount-ro", f"{HELPERS}:/probe", "-c", "test -e /probe/argv.py"],
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=30,
        check=False,
    )
    return probe.returncode == 0


# --- reporting ---------------------------------------------------------------


def pct(num, den):
    return round(100.0 * num / den, 1) if den else 0.0


def summarize(rows, with_bash):
    total = len(rows)
    bk = sum(1 for r in rows if r["bashkit"] is None)
    out = {"cases": total, "bashkit_pass": bk, "bashkit_pass_pct": pct(bk, total)}
    if with_bash:
        sh = [r for r in rows if r["bash"] is None]
        both = sum(1 for r in sh if r["bashkit"] is None)
        out.update(
            {
                "bash_pass": len(sh),
                "bash_pass_pct": pct(len(sh), total),
                "bashkit_pass_of_bash": both,
                "pass_rate": pct(both, len(sh)),
            }
        )
    else:
        out["pass_rate"] = out["bashkit_pass_pct"]
    return out


def write_markdown(path, report):
    t = report["total"]
    lines = [
        "# Oils spec pass rate",
        "",
        f"- Oils revision: [`{report['oils_rev'][:12]}`]({OILS_REPO}/tree/{report['oils_rev']})",
        f"- bashkit commit: `{report['bashkit_commit']}`",
        f"- bash: {report['bash_version'] or 'not run'}",
        f"- Spec files: {report['files_run']} (bash column; {report['files_skipped']} without one skipped)",
        f"- Runtime: {report['elapsed_secs']} s, {report['jobs']} jobs, {report['timeout_secs']} s per-case timeout",
        "",
        "## Headline",
        "",
    ]
    if "bash_pass" in t:
        lines += [
            (
                f"**{t['pass_rate']}%** — bashkit passes {t['bashkit_pass_of_bash']} of the "
                f"{t['bash_pass']} cases real bash passes."
            ),
            "",
            "| Shell | Pass | Cases | % |",
            "|-------|-----:|------:|--:|",
            f"| bash | {t['bash_pass']} | {t['cases']} | {t['bash_pass_pct']} |",
            f"| bashkit | {t['bashkit_pass']} | {t['cases']} | {t['bashkit_pass_pct']} |",
            "",
        ]
    else:
        lines += [
            f"**{t['pass_rate']}%** — bashkit passes {t['bashkit_pass']} of {t['cases']} cases (bash not run).",
            "",
        ]
    lines += [
        "## bashkit failures by class",
        "",
        "Heuristic keyword classes; every case still counts in the rate.",
        "",
        "| Class | Failing cases |",
        "|-------|--------------:|",
    ]
    for name, n in sorted(report["failure_classes"].items(), key=lambda kv: -kv[1]):
        lines.append(f"| {name} | {n} |")
    lines += [
        "",
        "## Per spec file",
        "",
        "Sorted by cases bashkit misses among those bash passes.",
        "",
        "| Spec file | Cases | bash | bashkit | Missed | Rate % |",
        "|-----------|------:|-----:|--------:|-------:|-------:|",
    ]
    for name, f in report["files"].items():
        s = f["summary"]
        missed = s.get("bash_pass", s["cases"]) - s.get("bashkit_pass_of_bash", s["bashkit_pass"])
        lines.append(
            f"| {name} | {s['cases']} | {s.get('bash_pass', '-')} | {s['bashkit_pass']} | {missed} | {s['pass_rate']} |"
        )
    lines.append("")
    path.write_text("\n".join(lines))


def git_commit():
    r = subprocess.run(
        ["git", "-C", str(ROOT), "rev-parse", "--short=12", "HEAD"], capture_output=True, text=True, check=False
    )
    return r.stdout.strip() or "unknown"


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--bin", default=os.environ.get("BASHKIT_BIN"), help="bashkit CLI built with realfs")
    ap.add_argument("--cache", default=os.environ.get("OILS_SPEC_CACHE", str(ROOT / "target" / "oils-spec")))
    ap.add_argument("--no-bash", action="store_true", help="skip real bash; rate becomes bashkit/all cases")
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    ap.add_argument("--timeout", type=int, default=10, help="per-case seconds")
    ap.add_argument("--save", action="store_true", help="write results/oils-spec-*.{json,md}")
    ap.add_argument("--fails", help="write failing cases (code, expected, actual) to this file")
    ap.add_argument("specs", nargs="*", help="spec names to run (e.g. arith var-sub); default all")
    args = ap.parse_args()

    if args.save and args.specs:
        sys.exit("oils-spec: --save records the whole suite; drop the spec names")
    binary = args.bin or str(ROOT / "target" / "debug" / "bashkit")
    if not os.access(binary, os.X_OK):
        sys.exit(f"oils-spec: no bashkit binary at {binary} (cargo build -p bashkit-cli --features realfs)")
    if not supports_mounts(binary):
        sys.exit(f"oils-spec: {binary} lacks --mount-ro; build with: cargo build -p bashkit-cli --features realfs")
    bash = None
    bash_version = None
    if not args.no_bash:
        bash = shutil.which("bash")
        if not bash:
            sys.exit("oils-spec: no bash on PATH (or pass --no-bash)")
        bash_version = subprocess.run(
            [bash, "-c", "echo $BASH_VERSION"], capture_output=True, text=True, check=False
        ).stdout.strip()

    repo = fetch_oils(pathlib.Path(args.cache))
    specs = sorted((repo / "spec").glob("*.test.sh"))
    if args.specs:
        want = {s.removesuffix(".test.sh") for s in args.specs}
        specs = [p for p in specs if p.name.removesuffix(".test.sh") in want]

    work = []
    skipped = 0
    for path in specs:
        meta, cases = parse_spec(path)
        if not in_bash_column(meta) or meta.get("suite") == "disabled":
            skipped += 1
            continue
        name = path.name.removesuffix(".test.sh")
        for i, case in enumerate(cases):
            work.append((name, i, case, bool(meta.get("legacy_tmp_dir"))))

    def run_one(item):
        name, i, case, legacy_tmp = item
        asserts = bash_assertions(case)
        bk = run_bashkit(binary, repo, case["code"], args.timeout, legacy_tmp)
        sh = run_bash(bash, repo, case["code"], args.timeout, legacy_tmp) if bash else None
        return name, i, case, asserts, bk, sh

    started = time.time()
    rows_by_file = {}
    fails = []
    done = 0
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for name, i, case, asserts, bk, sh in pool.map(run_one, work):
            row = {
                "case": i,
                "name": case["desc"],
                "qualifier": bash_qualifier(case),
                "bashkit": check(asserts, bk),
                "bash": check(asserts, sh) if sh else None,
            }
            if row["bashkit"] is not None:
                row["class"] = classify(case)
                if args.fails and row["bash"] is None:
                    fails.append((name, case, asserts, bk))
            rows_by_file.setdefault(name, []).append(row)
            done += 1
            if done % 200 == 0:
                print(f"oils-spec: {done}/{len(work)}", file=sys.stderr)
    elapsed = round(time.time() - started, 1)

    with_bash = bash is not None
    all_rows = [r for rows in rows_by_file.values() for r in rows]
    files = {}
    for name, rows in rows_by_file.items():
        s = summarize(rows, with_bash)
        files[name] = {
            "summary": s,
            # bashkit misses among cases bash passes: the list a PR shrinks.
            "missed": [
                {"case": r["case"], "name": r["name"], "class": r["class"], "on": r["bashkit"]}
                for r in rows
                if r["bashkit"] is not None and (r["bash"] is None or not with_bash)
            ],
            "bash_fails": [r["case"] for r in rows if with_bash and r["bash"] is not None],
        }
    key = (
        (lambda kv: (-(kv[1]["summary"]["bash_pass"] - kv[1]["summary"]["bashkit_pass_of_bash"]), kv[0]))
        if with_bash
        else (lambda kv: (-(kv[1]["summary"]["cases"] - kv[1]["summary"]["bashkit_pass"]), kv[0]))
    )
    files = dict(sorted(files.items(), key=key))
    classes = {}
    for r in all_rows:
        if r["bashkit"] is not None and (not with_bash or r["bash"] is None):
            classes[r["class"]] = classes.get(r["class"], 0) + 1

    report = {
        "schema": 1,
        "oils_repo": OILS_REPO,
        "oils_rev": OILS_REV,
        "bashkit_commit": git_commit(),
        "bash_version": bash_version,
        "timestamp": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platform": f"{platform.system().lower()}-{platform.machine()}",
        "jobs": args.jobs,
        "timeout_secs": args.timeout,
        "elapsed_secs": elapsed,
        "files_run": len(files),
        "files_skipped": skipped,
        "total": summarize(all_rows, with_bash),
        "failure_classes": classes,
        "files": files,
    }

    t = report["total"]
    if with_bash:
        print(
            f"oils-spec: {t['pass_rate']}% ({t['bashkit_pass_of_bash']}/{t['bash_pass']} of cases real bash passes); "
            f"bash {t['bash_pass']}/{t['cases']}, bashkit {t['bashkit_pass']}/{t['cases']}; {elapsed}s"
        )
    else:
        print(f"oils-spec: bashkit {t['bashkit_pass']}/{t['cases']} ({t['pass_rate']}%); {elapsed}s")

    if args.fails:
        with open(args.fails, "w", encoding="utf-8", errors="replace") as fh:
            for name, case, asserts, bk in fails:
                fh.write(f"==== {name} #{case['desc']}\n")
                fh.write(case["code"].decode("utf-8", "replace"))
                for key, expected, _ in asserts:
                    if bk.get("timeout"):
                        fh.write("-- TIMEOUT\n")
                        break
                    if bk[key] != expected:
                        fh.write(f"-- expected {key}: {expected!r}\n-- got      {key}: {bk[key]!r}\n")
                if not bk.get("timeout"):
                    fh.write(f"-- stderr: {bk['stderr'][:400]!r}\n")
                fh.write("\n")

    if args.save:
        RESULTS.mkdir(exist_ok=True)
        stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        base = RESULTS / f"oils-spec-{report['platform']}-{stamp}"
        base.with_suffix(".json").write_text(json.dumps(report, indent=1) + "\n")
        write_markdown(base.with_suffix(".md"), report)
        print(f"oils-spec: wrote {base}.json and .md")
    return 0


if __name__ == "__main__":
    sys.exit(main())
