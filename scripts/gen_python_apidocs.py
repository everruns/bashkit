#!/usr/bin/env python3
"""Generate the Python API reference markdown for bashkit.sh.

Decision: we have no docs.rs equivalent for the PyPI package, so we self-host
an API reference on bashkit.sh. To stay on-brand we emit plain markdown and let
the Astro `DocsLayout` render it (same colors/typography/Shiki theme as every
other doc page) instead of shipping a foreign pdoc/Sphinx theme.

Source of truth: the PEP 561 type stubs in `crates/bashkit-python/bashkit`
(`_bashkit.pyi` + the pure-Python integration modules). We use `griffe` to
statically parse them (no compiled extension or third-party deps required,
`allow_inspection=False`), then render markdown. Latest-only: regenerated on
release, output committed to the repo so the node-only site build just renders
it. See specs/documentation.md ("API reference hosting").

Usage: python3 scripts/gen_python_apidocs.py
"""

from __future__ import annotations

import sys
from pathlib import Path

import griffe

REPO_ROOT = Path(__file__).resolve().parent.parent
PKG_SEARCH_PATH = REPO_ROOT / "crates" / "bashkit-python"
OUT_PATH = REPO_ROOT / "site" / "src" / "content" / "apidocs" / "python.md"

# Curated public surface, in rendering order. Mirrors bashkit.__all__ plus the
# framework integration modules that ship in the package.
CORE_ORDER = [
    "Bash",
    "BashTool",
    "ScriptedTool",
    "FileSystem",
    "ExecResult",
    "ShellState",
    "BuiltinContext",
    "BuiltinResult",
    "BashError",
    "create_langchain_tool_spec",
    "get_version",
]

INTEGRATIONS = [
    ("bashkit.langchain", "bashkit.langchain"),
    ("bashkit.pydantic_ai", "bashkit.pydantic_ai"),
    ("bashkit.deepagents", "bashkit.deepagents"),
]


def annotation_str(ann) -> str:
    if ann is None:
        return ""
    return str(ann)


def render_signature(name: str, func) -> str:
    """Build a Python-style signature string for a function/method."""
    parts = []
    for p in func.parameters:
        if p.name == "self":
            continue
        kind = getattr(p.kind, "value", "")
        prefix = ""
        if kind == "variadic positional":
            prefix = "*"
        elif kind == "variadic keyword":
            prefix = "**"
        piece = f"{prefix}{p.name}"
        ann = annotation_str(p.annotation)
        if ann:
            piece += f": {ann}"
        if p.default is not None and not prefix:
            piece += f" = {p.default}"
        parts.append(piece)
    ret = annotation_str(func.returns)
    arrow = f" -> {ret}" if ret else ""
    return f"{name}({', '.join(parts)}){arrow}"


def docstring_of(obj) -> str:
    if obj.docstring and obj.docstring.value:
        return obj.docstring.value.strip()
    return ""


def own_members(obj):
    return {n: m for n, m in obj.members.items() if not m.is_alias}


def render_function(name: str, func, *, heading: str, level: int) -> list[str]:
    lines = [f"{'#' * level} {heading}", ""]
    lines.append("```python")
    lines.append(render_signature(name, func))
    lines.append("```")
    lines.append("")
    doc = docstring_of(func)
    if doc:
        lines.append(doc)
        lines.append("")
    return lines


def render_class(name: str, cls) -> list[str]:
    lines = [f"## {name}", ""]
    doc = docstring_of(cls)
    if doc:
        lines.append(doc)
        lines.append("")

    members = own_members(cls)
    methods = [
        (n, m)
        for n, m in members.items()
        if m.is_function and (n == "__init__" or not n.startswith("_"))
    ]
    attributes = [
        (n, m)
        for n, m in members.items()
        if m.is_attribute and not n.startswith("_")
    ]

    if attributes:
        lines.append("### Fields")
        lines.append("")
        for n, attr in attributes:
            ann = annotation_str(attr.annotation)
            suffix = f" — `{ann}`" if ann else ""
            adoc = docstring_of(attr)
            adoc = f" {adoc}" if adoc else ""
            lines.append(f"- **`{n}`**{suffix}{adoc}")
        lines.append("")

    for n, meth in methods:
        heading = "Constructor" if n == "__init__" else f"`{n}`"
        sig_name = name if n == "__init__" else f"{name}.{n}"
        lines += render_function(sig_name, meth, heading=heading, level=3)

    return lines


def main() -> int:
    pkg = griffe.load(
        "bashkit",
        search_paths=[str(PKG_SEARCH_PATH)],
        allow_inspection=False,
    )
    native = pkg["_bashkit"]

    out: list[str] = []
    out.append("# Python API reference")
    out.append("")
    out.append(
        "Auto-generated reference for the [`bashkit`](https://pypi.org/project/bashkit/) "
        "PyPI package, covering the public classes and functions exported from "
        "`bashkit`. Reflects the latest published release."
    )
    out.append("")
    out.append(
        "> Install with `pip install bashkit`. See the "
        "[Embedding guide](/docs/embedding/) and "
        "[LLM tools guide](/docs/llm-tools/) for task-oriented walkthroughs."
    )
    out.append("")

    for name in CORE_ORDER:
        member = native.members.get(name)
        if member is None:
            print(f"warning: {name} not found in stubs", file=sys.stderr)
            continue
        if member.is_class:
            out += render_class(name, member)
        elif member.is_function:
            out += render_function(
                name, member, heading=f"{name}()", level=2
            )

    # Framework integration modules (pure-Python).
    integ_lines: list[str] = []
    for mod_path, title in INTEGRATIONS:
        try:
            mod = pkg
            for part in mod_path.split(".")[1:]:
                mod = mod[part]
        except KeyError:
            continue
        funcs = [
            (n, m)
            for n, m in own_members(mod).items()
            if m.is_function and not n.startswith("_")
        ]
        if not funcs:
            continue
        integ_lines.append(f"## `{title}`")
        integ_lines.append("")
        mdoc = docstring_of(mod)
        if mdoc:
            integ_lines.append(mdoc.splitlines()[0])
            integ_lines.append("")
        for n, fn in funcs:
            integ_lines += render_function(
                f"{title}.{n}", fn, heading=f"`{n}`", level=3
            )

    if integ_lines:
        out.append("---")
        out.append("")
        out.append("# Framework integrations")
        out.append("")
        out += integ_lines

    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    text = "\n".join(out).rstrip() + "\n"
    OUT_PATH.write_text(text, encoding="utf-8")
    print(f"wrote {OUT_PATH.relative_to(REPO_ROOT)} ({len(text)} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
