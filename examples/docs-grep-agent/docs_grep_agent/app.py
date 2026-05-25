from __future__ import annotations

import argparse
import asyncio
import os
import re
import sys
from pathlib import Path

from langchain.agents import create_agent
from langchain_core.messages import HumanMessage
from langchain_openai import ChatOpenAI

from bashkit.langchain import create_bash_tool

DEFAULT_MODEL = "gpt-5.5-low"
# Public docs chat should retrieve snippets first; this cap is a backstop, not
# a substitute for narrow searches.
MAX_CONTEXT_CHARS = 8000
SPINNER_INTERVAL_SECONDS = 0.25
EXCLUDED_EXAMPLE_FILES = {"package-lock.json", "uv.lock"}

SYSTEM_PROMPT = """You answer questions about Bashkit from documentation snippets.

Bashkit basics:
- Bashkit is a sandboxed virtual Bash interpreter for running untrusted scripts.
- It uses a virtual filesystem and resource limits instead of direct host shell access.
- Python bindings expose the Rust core in-process, including the LangChain bash tool.
- Real host folders can be mounted into the sandbox; these docs are mounted read-only.

Mounted entrypoints:
- /docs/public: public docs such as cli.md, security.md, snapshotting.md, and builtin guides.
- /docs/rustdoc: Rustdoc markdown guides for builtins, VFS, hooks, Python, TypeScript, SQLite, SSH, and compatibility.
- /docs/examples: curated runnable example files, selected example READMEs, and selected example source directories.

Rules:
- Call the bash tool before answering Bashkit documentation questions.
- Write the bash script yourself.
- Use one multi-command bash script, or make multiple bash tool calls when separate searches are useful.
- Only inspect files under /docs/public, /docs/rustdoc, and /docs/examples.
- Generated local artifacts such as .venv, .ruff_cache, __pycache__, and lockfiles are intentionally not mounted.
- Keep tool output compact. Bound every broad search with `head`, `-m`, `-l`, or a narrow `sed -n` range, and aim to keep total tool output under 8000 characters.
- Search progressively:
  1. Use `rg -i -n PATTERN PATH... | head -20` for broad discovery.
  2. Use `rg -i -l PATTERN PATH... | head -10` before reading content for broad questions.
  3. Use `grep -R -i -n -C 1 -m 3 -- PATTERN PATH...` when you need context around matches.
  4. Use `sed -n 'START,ENDp' FILE` after finding the best file and line range.
- Never use `cat` for docs or examples. Read focused excerpts with `sed -n`, and keep each range under 120 lines.
- Use `grep` instead of `rg` when you need context flags (`-A`, `-B`, `-C`) or include/exclude filters. Bashkit `rg` is intentionally simpler than full ripgrep.
- Use `-F` with `rg` or `grep` for exact strings.
- For multi-topic questions, print short section labels and run one compact search per topic.
- Do not act as a filesystem browser. If the user asks to list, tree, enumerate, dump, or browse directories/files, do not return raw listings. Say that the agent answers Bashkit docs questions and mention the mounted entrypoint categories instead.
- Use ls/find only as an internal discovery step for a specific Bashkit docs question, not as the final answer.
- Use only facts present in bash tool output.
- Do not treat a failed command as proof that something is absent; retry with a simpler command when needed.
- Bashkit find supports common simple predicates but not every GNU find expression; prefer separate `find ... -name PATTERN` calls over `-o`.
- If a successful command produces no output, say that it produced no output instead of returning an empty answer.
- Keep answers concise and practical.
- For CLI or code questions, search the relevant docs and examples, then include a runnable example when the output contains one.
- If the bash output does not answer the question, say the docs snippets do not show it.
"""


def parse_model(value: str) -> tuple[str, str | None]:
    """Public shorthand keeps the CLI fast to type while using OpenAI reasoning config."""
    match = re.match(r"^(gpt-.+)-(low|medium|high|none|minimal)$", value)
    if match:
        return match.group(1), match.group(2)
    return value, None


def repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def build_example_files(root: Path) -> dict[str, str]:
    examples = root / "examples"
    files = {
        f"/docs/examples/{path.name}": path.read_text(encoding="utf-8")
        for path in sorted(examples.iterdir())
        if path.is_file() and path.name not in EXCLUDED_EXAMPLE_FILES
    }
    for rel in [
        "docs-grep-agent/.gitignore",
        "docs-grep-agent/README.md",
        "docs-grep-agent/pyproject.toml",
        "docs-grep-agent/docs_grep_agent/__init__.py",
        "docs-grep-agent/docs_grep_agent/__main__.py",
        "docs-grep-agent/docs_grep_agent/app.py",
    ]:
        path = examples / rel
        if path.exists():
            files[f"/docs/examples/{rel}"] = path.read_text(encoding="utf-8")
    return files


def create_docs_bash_tool(root: Path):
    docs_mounts = [
        (root / "docs", "/docs/public"),
        (root / "crates/bashkit/docs", "/docs/rustdoc"),
    ]
    return create_bash_tool(
        username="agent",
        hostname="docs",
        max_commands=120,
        max_loop_iterations=1000,
        timeout_seconds=3,
        mounts=[
            {"host_path": str(host_path), "vfs_path": vfs_path, "writable": False}
            for host_path, vfs_path in docs_mounts
        ],
        files=build_example_files(root),
        allowed_mount_paths=[str(host_path) for host_path, _ in docs_mounts],
        readonly_filesystem=True,
        max_output_length=MAX_CONTEXT_CHARS,
    )


def approx_tokens(text: str) -> int:
    return (len(text) + 3) // 4


async def spinner(stop: asyncio.Event) -> None:
    label = "preparing"
    clear = "\r" + (" " * (len(label) + 2)) + "\r"
    try:
        while not stop.is_set():
            for frame in ("*", " "):
                if stop.is_set():
                    break
                print(f"\r{label} {frame}", end="", file=sys.stderr, flush=True)
                try:
                    await asyncio.wait_for(stop.wait(), SPINNER_INTERVAL_SECONDS)
                except TimeoutError:
                    pass
    finally:
        print(clear, end="", file=sys.stderr, flush=True)


async def answer(
    question: str,
    model: str,
    show_tools: bool,
    root: Path,
    show_spinner: bool,
) -> None:
    model_name, reasoning_effort = parse_model(model)
    llm_kwargs = {"model": model_name, "streaming": True}
    if reasoning_effort:
        llm_kwargs["reasoning"] = {"effort": reasoning_effort}

    agent = create_agent(
        model=ChatOpenAI(**llm_kwargs),
        tools=[create_docs_bash_tool(root)],
        system_prompt=SYSTEM_PROMPT,
    )
    printed = False
    spinner_stop = asyncio.Event()
    spinner_task = asyncio.create_task(spinner(spinner_stop)) if show_spinner else None

    async def stop_spinner() -> None:
        if spinner_task and not spinner_stop.is_set():
            spinner_stop.set()
            await spinner_task

    async def print_answer_text(text: str) -> None:
        nonlocal printed
        if not text:
            return
        if not printed:
            await stop_spinner()
        print(text, end="", flush=True)
        printed = True

    try:
        async for event in agent.astream_events(
            {"messages": [HumanMessage(content=question)]},
            version="v2",
        ):
            if show_tools and event["event"] == "on_tool_start":
                commands = event["data"].get("input", {}).get("commands")
                if commands:
                    await stop_spinner()
                    print(f"$ {commands}", file=sys.stderr, flush=True)
            elif event["event"] == "on_chat_model_stream":
                content = event["data"]["chunk"].content
                if isinstance(content, str):
                    await print_answer_text(content)
                elif isinstance(content, list):
                    for block in content:
                        if isinstance(block, dict) and block.get("type") in {
                            "text",
                            "output_text",
                        }:
                            await print_answer_text(str(block.get("text", "")))
    finally:
        await stop_spinner()

    if printed:
        print()


def self_test(root: Path) -> None:
    bash_tool = create_docs_bash_tool(root)
    what = bash_tool.invoke(
        {"commands": "rg -i -n 'bashkit' /docs/public /docs/rustdoc | head -10"}
    )
    cli = bash_tool.invoke(
        {"commands": "grep -R -i -n -C 1 -m 3 -- 'cli' /docs/public"}
    )
    bounded = bash_tool.invoke(
        {
            "commands": "yes bashkit | head -20000",
        }
    )
    readonly = bash_tool.invoke({"commands": "printf nope > /docs/public/nope.txt"})
    copy = bash_tool.invoke({"commands": "cp /docs/public/cli.md /tmp/cli-copy.md"})
    examples = bash_tool.invoke(
        {"commands": "find /docs/examples -maxdepth 2 -type f | head"}
    )
    shortlist = bash_tool.invoke(
        {
            "commands": "rg -i -l 'cli' /docs/public /docs/rustdoc /docs/examples | head -10"
        }
    )
    focused_cli = bash_tool.invoke({"commands": "sed -n '1,80p' /docs/public/cli.md"})
    full_cli = bash_tool.invoke({"commands": "cat /docs/public/cli.md"})
    generated = bash_tool.invoke(
        {
            "commands": (
                "find /docs/examples -maxdepth 4 -name .venv -print; "
                "find /docs/examples -maxdepth 4 -name .ruff_cache -print; "
                "find /docs/examples -maxdepth 4 -name __pycache__ -print; "
                "find /docs/examples -maxdepth 4 -name uv.lock -print; "
                "find /docs/examples -maxdepth 4 -name package-lock.json -print"
            )
        }
    )

    assert "sandboxed" in what.lower() or "bashkit" in what.lower()
    assert "bashkit-cli" in cli.lower()
    assert len(bounded) <= MAX_CONTEXT_CHARS + len("\n[truncated]")
    assert bounded.endswith("[truncated]")
    assert "[Exit code:" in readonly
    assert "[Exit code:" in copy
    assert "/docs/examples/" in examples
    assert len(shortlist) < 1000
    assert len(focused_cli) < len(full_cli)
    assert generated.strip() == ""
    print(
        "token-efficiency "
        f"shortlist~{approx_tokens(shortlist)}t "
        f"focused~{approx_tokens(focused_cli)}t "
        f"full-file~{approx_tokens(full_cli)}t "
        f"avoided~{approx_tokens(full_cli) - approx_tokens(focused_cli)}t"
    )
    print("self-test ok")


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Answer Bashkit docs questions with a LangGraph + bashkit search agent."
    )
    parser.add_argument(
        "question", nargs="*", help="Question to answer. Omit for interactive mode."
    )
    parser.add_argument(
        "--model", default=os.getenv("BASHKIT_DOCS_MODEL", DEFAULT_MODEL)
    )
    parser.add_argument(
        "--docs-root",
        type=Path,
        default=repo_root(),
        help="Repository root containing Bashkit docs.",
    )
    parser.add_argument(
        "--show-tools",
        action="store_true",
        help="Print one-line bash search scripts to stderr.",
    )
    parser.add_argument(
        "--no-spinner",
        action="store_true",
        help="Disable the in-place preparation spinner.",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="Run a local smoke test without an OpenAI API key.",
    )
    return parser.parse_args(argv)


def main() -> None:
    argv = sys.argv[1:]
    args = parse_args(argv)
    root = args.docs_root.resolve()

    if args.self_test:
        self_test(root)
        return

    if not os.getenv("OPENAI_API_KEY"):
        raise SystemExit(
            "OPENAI_API_KEY is required for model-backed answers. Run --self-test for the local smoke test."
        )

    if args.question:
        try:
            asyncio.run(
                answer(
                    " ".join(args.question),
                    args.model,
                    args.show_tools,
                    root,
                    not args.no_spinner,
                )
            )
        except KeyboardInterrupt:
            print()
        return

    while True:
        try:
            question = input("bashkit docs> ").strip()
        except (EOFError, KeyboardInterrupt):
            print()
            return
        if not question or question.lower() in {"exit", "quit"}:
            return
        try:
            asyncio.run(
                answer(question, args.model, args.show_tools, root, not args.no_spinner)
            )
        except KeyboardInterrupt:
            print()
            return
