"""Deterministic CHANGELOG release-section checks.

Lock the v0.18.0 release-page drift: a stale `#2347` highlight carried over
from v0.17.1, a verbatim-duplicated C ABI bullet (Highlights + Added), and
ten curated bullets with no PR link (the #2378 maintenance scope).

Convention C: the release section holds curated `### Highlights` plus an
exhaustive `### What's Changed` -- no Added/Fixed/Changed subsections (their
entries duplicate What's Changed by construction).

Rules (top `## [...]` section only):
  R0 every PR link is well-formed: `[#N](.../pull/N)` with matching numbers.
  R1 every Highlights bullet carries a PR link.
  R2 no two Highlights bullets share normalized text (links stripped, case and
     whitespace folded, substring containment included).
  R3 no curated PR link may already appear in an older `## [x.y.z]` section
     (stale carry-over from a previous release).
  R4 no subsection other than Highlights / What's Changed may exist.

`--scaffold PREV_TAG` prints a deterministic `## What's Changed` block built
from `git log PREV_TAG..HEAD` merge order (same order GitHub generates).
"""
import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CHANGELOG = ROOT / "CHANGELOG.md"
PULL_URL = "https://github.com/everruns/bashkit/pull/"

SECTION_RE = re.compile(r"^## \[([^\]]+)\]")
SUBSECTION_RE = re.compile(r"^### (.+?)\s*$")
LINK_RE = re.compile(r"\[#(\d+)\]\(https://github\.com/everruns/bashkit/pull/(\d+)\)")
PR_SUFFIX_RE = re.compile(r"^(.*)\s+\(#(\d+)\)\s*$")


def parse_sections(text):
    """Split markdown into [(title, lines)] at `## [...]` headers."""
    sections, title, buf = [], None, []
    for line in text.splitlines():
        m = SECTION_RE.match(line)
        if m:
            if title is not None:
                sections.append((title, buf))
            title, buf = m.group(1), []
        elif title is not None:
            buf.append(line)
    if title is not None:
        sections.append((title, buf))
    return sections


def iter_bullets(lines):
    """Yield (subsection, text) joining wrapped continuation lines."""
    sub, cur = "top", None
    for line in lines + [""]:
        m = SUBSECTION_RE.match(line)
        if m:
            if cur is not None:
                yield sub, " ".join(cur)
                cur = None
            sub = m.group(1)
            continue
        if line.startswith("- "):
            if cur is not None:
                yield sub, " ".join(cur)
            cur = [line[2:]]
        elif cur is not None and line.startswith(" ") and line.strip():
            cur.append(line.strip())
        elif cur is not None and not line.strip():
            yield sub, " ".join(cur)
            cur = None
    if cur is not None:
        yield sub, " ".join(cur)


LINK_SENTENCE_RE = re.compile(
    r"\(\[#\d+\]\(https://github\.com/everruns/bashkit/pull/\d+\)\)\.?"
)


def normalize(text):
    text = LINK_SENTENCE_RE.sub("", text)  # link (+ trailing period) is not prose
    return re.sub(r"\s+", " ", text).strip().lower()


def check_section(title, lines, older_prs):
    """Return list of violation strings for one section."""
    errors = []
    seen = {}
    for sub, bullet in iter_bullets(lines):
        if sub == "What's Changed":
            continue
        links = LINK_RE.findall(bullet)
        for text_n, url_n in links:
            if text_n != url_n:
                errors.append(f"[{title}] malformed link (text #{text_n} != url #{url_n}): {bullet[:80]}")
        if not links:
            errors.append(f"[{title}] curated bullet without PR link ({sub}): {bullet[:80]}")
            continue
        norm = normalize(bullet)
        if norm in seen:
            errors.append(f"[{title}] duplicate bullet text ({sub}, first in {seen[norm]}): {bullet[:80]}")
        else:
            dup = next((prev for prev in seen
                        if len(norm) >= 30 and len(prev) >= 30
                        and (norm in prev or prev in norm)), None)
            if dup is not None:
                errors.append(f"[{title}] duplicate bullet text ({sub}, first in {seen[dup]}): {bullet[:80]}")
            else:
                seen[norm] = sub
        for text_n, _ in links:
            if text_n in older_prs:
                errors.append(f"[{title}] stale PR #{text_n} already released in {older_prs[text_n]}: {bullet[:80]}")
    return errors


def collect_prs(lines):
    """Map PR number -> True for every PR link in section lines."""
    prs = {}
    for _, bullet in iter_bullets(lines):
        for text_n, _ in LINK_RE.findall(bullet):
            prs[text_n] = True
    return prs


def check_changelog(text):
    sections = parse_sections(text)
    if not sections:
        return ["no ## [version] sections found"]
    errors = []
    older_prs = {}
    for title, lines in reversed(sections[1:]):
        if re.fullmatch(r"\d+\.\d+\.\d+", title):
            for n in collect_prs(lines):
                older_prs.setdefault(n, title)
    title, lines = sections[0]
    subs = [m.group(1) for line in lines if (m := SUBSECTION_RE.match(line))]
    for sub in subs:
        if sub not in ("Highlights", "What's Changed"):
            errors.append(f"[{title}] unexpected subsection '### {sub}' (use Highlights + What's Changed only)")
    errors.extend(check_section(title, lines, older_prs))
    return errors


def build_whats_changed(subjects):
    """Pure: subjects -> What's Changed lines, merge order kept, PR-less dropped."""
    lines = ["## What's Changed", ""]
    seen = set()
    for subject in subjects:
        m = PR_SUFFIX_RE.match(subject)
        if not m:
            continue  # direct push, no PR: GitHub omits it too
        title, num = m.group(1).strip(), m.group(2)
        if num in seen:
            continue
        seen.add(num)
        lines.append(f"* {title} in [#{num}]({PULL_URL}{num})")
    return lines


def scaffold(prev_tag, end="HEAD"):
    out = subprocess.run(
        ["git", "-C", str(ROOT), "log", f"{prev_tag}..{end}", "--pretty=format:%s"],
        capture_output=True, text=True, check=True,
    ).stdout.splitlines()
    print("\n".join(build_whats_changed(out)))


def main(argv=None):
    ap = argparse.ArgumentParser(description="CHANGELOG release-section checks")
    ap.add_argument("--scaffold", metavar="PREV_TAG",
                    help="print deterministic What's Changed block since tag")
    ap.add_argument("--end", default="HEAD", help="range end for --scaffold")
    ap.add_argument("path", nargs="?", default=str(CHANGELOG))
    args = ap.parse_args(argv)
    if args.scaffold:
        scaffold(args.scaffold, args.end)
        return 0
    errors = check_changelog(Path(args.path).read_text())
    for e in errors:
        print(f"check_changelog: {e}", file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
