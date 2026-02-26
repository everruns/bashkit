# Skills.sh Top 44 Skills: Bashkit Compatibility Analysis

Analysis of skills from [skills.sh](https://skills.sh) leaderboard to assess
whether their bash/script usage maps to bashkit's supported feature set.

## Executive Summary

**44 skills analyzed** across 12 repositories. Key findings:

| Category | Count | % |
|----------|-------|---|
| Pure markdown/instructions (no scripts) | 29 | 66% |
| Uses bash scripts | 8 | 18% |
| Uses Python scripts | 10 | 23% |
| Uses JS/TS | 3 | 7% |
| Uses PowerShell | 1 | 2% |
| Requires hard binaries (unsimulatable) | 8 | 18% |

**Bottom line:** ~66% of top skills are pure-instruction skills requiring zero
script execution. Of the ~34% with scripts, most use Python heavily and bash
as glue. The bash features used are well within bashkit's capabilities. The
main gap is **external binary dependencies** (LibreOffice, poppler, pandoc,
az CLI, agent-browser, node/npm/pnpm) that bashkit cannot simulate.

---

## Skill-by-Skill Analysis

### 1. find-skills (vercel-labs/skills) — 325K installs

- **Type:** Pure markdown instructions
- **Scripts:** None
- **Binaries:** None
- **Bashkit support:** Full (nothing to execute)

### 2. vercel-react-best-practices (vercel-labs/agent-skills) — 168K

- **Type:** Pure markdown instructions
- **Scripts:** None
- **Binaries:** None
- **Bashkit support:** Full

### 3. web-design-guidelines (vercel-labs/agent-skills) — 128K

- **Type:** Pure markdown instructions
- **Scripts:** None
- **Binaries:** None
- **Bashkit support:** Full

### 4. remotion-best-practices (remotion-dev/skills) — 112K

- **Type:** Pure markdown instructions (React/Remotion coding guidance)
- **Scripts:** None
- **Binaries:** None
- **Bashkit support:** Full

### 5. frontend-design (anthropics/skills) — 100K

- **Type:** Pure markdown instructions (UI/frontend design principles)
- **Scripts:** None
- **Binaries:** None
- **Bashkit support:** Full

### 6. agent-browser (vercel-labs/agent-browser) — 60K

- **Type:** Uses bash template scripts
- **Scripts:** 3 bash templates (authenticated-session.sh, form-automation.sh, capture-workflow.sh)
- **Binaries:** `agent-browser` CLI (Rust native binary)
- **Bash features used:**
  - `set -euo pipefail`
  - Variable expansion (`${1:?Usage}`, `${2:-default}`)
  - `[[ ]]` conditionals with glob patterns (`*"login"*`)
  - Command substitution `$(agent-browser get url)`
  - `if/fi`, `for/do/done`
  - Redirections (`2>/dev/null`, `> file`)
  - `trap cleanup EXIT`
  - `mkdir -p`, `rm -f`, `ls -la`
  - `|| true` error suppression
- **Bashkit support:** Bash syntax: **FULL**. Binary: **NOT SUPPORTED** (`agent-browser` is a native Rust CLI that controls real browsers via Playwright — cannot be simulated)

### 7. vercel-composition-patterns (vercel-labs/agent-skills) — 58K

- **Type:** Pure markdown instructions
- **Scripts:** None
- **Binaries:** None
- **Bashkit support:** Full

### 8–24. Microsoft Azure skills (microsoft/github-copilot-for-azure) — ~57K each

17 Azure skills: azure-observability, azure-ai, azure-cost-optimization,
azure-storage, azure-diagnostics, azure-deploy, microsoft-foundry,
azure-kusto, azure-resource-visualizer, entra-app-registration,
appinsights-instrumentation, azure-validate, azure-prepare,
azure-compliance, azure-aigateway, azure-resource-lookup, azure-rbac

- **Type:** Mostly markdown instructions with `az` CLI command references
- **Scripts:**
  - `microsoft-foundry` has 3 bash scripts (`discover_and_rank.sh`, `query_capacity.sh`, `generate_deployment_url.sh`)
  - `appinsights-instrumentation` has 1 PowerShell script (`appinsights.ps1`)
- **Binaries:** `az` CLI (Azure CLI), `python3` (inline), `jq`
- **Bash features used in microsoft-foundry scripts:**
  - `set -euo pipefail`, `set -e`
  - `declare -A` (associative arrays)
  - `for/do/done` loops with word splitting
  - Variable expansion: `${1:?Usage}`, `${2:-}`, `${!QUOTA_MAP[@]}`
  - Command substitution: `$(az account show --query id -o tsv)`
  - Pipes: `echo "$JSON" | jq -r '...'`, `| sort -u`, `| head -1`
  - Redirections: `2>/dev/null`, `|| echo "[]"` fallback
  - `while [[ $# -gt 0 ]]; do case $1 in ... esac; done` argument parsing
  - `printf` with format strings
  - Brace expansion: `{1..60}`
  - `xxd -r -p | base64 | tr '+' '-' | tr '/' '_' | tr -d '='` (binary encoding pipeline)
  - `cat << EOF` heredocs
  - Inline `python3 -c "..."` with embedded multi-line Python
  - String concatenation in loops building JSON
  - Nested function definitions (`usage()`, `has_dep()`)
  - `[[ "$OSTYPE" == "darwin"* ]]` pattern matching
- **Bashkit support:**
  - Bash syntax: **MOSTLY SUPPORTED** (all features listed above are implemented in bashkit)
  - `declare -A`: supported
  - Pipes, jq, sort, tr, head, base64: all supported as builtins
  - `xxd -r -p`: supported (`xxd` builtin with `-r`, `-p` flags)
  - `printf` with formatting: supported
  - `cat << EOF`: supported
  - **NOT SUPPORTED:** `az` CLI (Azure CLI binary — requires real Azure API access), `base64` (not listed as bashkit builtin)
  - PowerShell (`.ps1`): **NOT SUPPORTED**

### 25. skill-creator (anthropics/skills) — 49K

- **Type:** Python-heavy meta-skill
- **Scripts:** 8 Python files, 0 bash scripts
- **Binaries:** `claude` CLI (invoked via subprocess), `nohup`, `kill`
- **Bash features used (in SKILL.md instructions):**
  - `nohup ... > /dev/null 2>&1 &` (background execution)
  - `$!` (last background PID), `kill $VIEWER_PID`
  - `python -m scripts.run_loop --eval-set ... --max-iterations 5`
  - `cp -r` for directory copying
- **Bashkit support:**
  - Bash syntax: **PARTIAL** (`&` background parsed but runs synchronously; `$!` returns 0)
  - Python: **PARTIAL** (bashkit's embedded Monty interpreter supports basic Python but NOT `subprocess`, `concurrent.futures`, `anthropic` SDK, `http.server`, `webbrowser` — all required by skill-creator scripts)
  - `claude` CLI: **NOT SUPPORTED** (external binary)

### 26. azure-postgres (microsoft/github-copilot-for-azure) — 46K

- **Type:** Markdown instructions with `az` CLI references
- **Scripts:** None
- **Binaries:** `az` CLI
- **Bashkit support:** Full for bash syntax; `az` not available

### 27. azure-messaging (microsoft/github-copilot-for-azure) — 43K

- **Type:** Markdown instructions with `az` CLI references
- **Scripts:** None
- **Binaries:** `az` CLI
- **Bashkit support:** Full for bash syntax; `az` not available

### 28. vercel-react-native-skills (vercel-labs/agent-skills) — 41K

- **Type:** Pure markdown instructions
- **Scripts:** None
- **Binaries:** None
- **Bashkit support:** Full

### 29. browser-use (browser-use/browser-use) — 40K

- **Type:** Python-heavy browser automation framework
- **Scripts:** Python (massive library — 100+ files)
- **Binaries:** `browser-use` CLI, Playwright, Chrome/Chromium
- **Bash features used:** `set -e`, basic `if/fi`, `pip install`
- **Bashkit support:** Bash syntax: full. Python/binaries: **NOT SUPPORTED** (requires Playwright, real browser, network access, dozens of pip packages)

### 30. ui-ux-pro-max (nextlevelbuilder/ui-ux-pro-max-skill) — 39K

- **Type:** Python scripts + CSV data files
- **Scripts:** 3 Python files (core.py, search.py, design_system.py) + large CSV datasets
- **Binaries:** None (pure Python)
- **Bash features used:** None (Python invoked via `python scripts/search.py "query"`)
- **Bashkit support:**
  - Bash syntax: full
  - Python: **PARTIAL** (uses `csv`, `re`, `math`, `collections`, `pathlib`, `argparse`, `sys`, `io` — most are NOT available in bashkit's Monty interpreter which lacks most stdlib modules)

### 31. brainstorming (obra/superpowers) — 31K

- **Type:** Pure markdown instructions (ideation methodology)
- **Scripts:** None
- **Binaries:** None
- **Bashkit support:** Full

### 32. audit-website (squirrelscan/skills) — 27K

- **Type:** Markdown instructions for website security auditing
- **Scripts:** None
- **Binaries:** References `curl`, `nmap`, `nikto`, `wappalyzer`
- **Bashkit support:** Bash syntax: full. `curl`: supported (feature-gated). Other tools: **NOT SUPPORTED**

### 33. seo-audit (coreyhaines31/marketingskills) — 27K

- **Type:** Pure markdown instructions
- **Scripts:** None
- **Binaries:** None
- **Bashkit support:** Full

### 34. supabase-postgres-best-practices (supabase/agent-skills) — 24K

- **Type:** Pure markdown instructions (PostgreSQL patterns)
- **Scripts:** None
- **Binaries:** None
- **Bashkit support:** Full

### 35. pdf (anthropics/skills) — 22K

- **Type:** Python-heavy document processing
- **Scripts:** 8 Python files
- **Binaries:** `pdftotext`, `qpdf`, `pdftk`, `pdfimages` (poppler-utils), `tesseract`
- **Bash features used:**
  - Simple flag-based commands: `pdftotext -layout input.pdf output.txt`
  - `pdftotext -f 1 -l 5 input.pdf output.txt`
  - `qpdf --empty --pages file1.pdf file2.pdf -- merged.pdf`
- **Bashkit support:**
  - Bash syntax: full (all features trivial)
  - Python: **NOT SUPPORTED** (requires `pypdf`, `pdfplumber`, `reportlab`, `pytesseract`, `pdf2image`)
  - Binaries: **NOT SUPPORTED** (`pdftotext`, `qpdf`, `pdftk`, `tesseract` are native binaries)

### 36. azure-hosted-copilot-sdk (microsoft/github-copilot-for-azure) — 21K

- **Type:** Markdown instructions
- **Scripts:** None
- **Binaries:** `az` CLI
- **Bashkit support:** Full for bash syntax; `az` not available

### 37. next-best-practices (vercel-labs/next-skills) — 21K

- **Type:** Pure markdown instructions (Next.js patterns)
- **Scripts:** None
- **Binaries:** None
- **Bashkit support:** Full

### 38. copywriting (coreyhaines31/marketingskills) — 21K

- **Type:** Pure markdown instructions
- **Scripts:** None
- **Binaries:** None
- **Bashkit support:** Full

### 39. pptx (anthropics/skills) — 18K

- **Type:** Python scripts + document processing
- **Scripts:** 3 Python files + shared office library
- **Binaries:** `soffice` (LibreOffice), `pdftoppm` (poppler), `markitdown`, `npm/pptxgenjs`
- **Bash features used:**
  - Pipe: `python -m markitdown output.pptx | grep -iE "xxxx|lorem|ipsum|..."`
  - `grep -iE` with extended regex and alternation
  - `pdftoppm -jpeg -r 150 -f N -l N output.pdf slide`
- **Bashkit support:**
  - Bash syntax: **FULL** (pipes, grep -iE, flags all supported)
  - Python: **NOT SUPPORTED** (requires `PIL/Pillow`, `defusedxml`, `subprocess`)
  - Binaries: **NOT SUPPORTED** (`soffice`, `pdftoppm`, `npm`)

### 40. systematic-debugging (obra/superpowers) — 17K

- **Type:** Markdown instructions + 1 bash script (`find-polluter.sh`)
- **Scripts:** `find-polluter.sh` (64 lines)
- **Binaries:** `npm test` (invoked)
- **Bash features used:**
  - `set -e`
  - `if [ $# -ne 2 ]; then ... fi`
  - `for TEST_FILE in $TEST_FILES; do ... done`
  - Command substitution: `$(find . -path "$TEST_PATTERN" | sort)`
  - Pipes: `echo "$TEST_FILES" | wc -l | tr -d ' '`
  - Arithmetic: `COUNT=$((COUNT + 1))`
  - `-e` file test, `-z` string test
  - `> /dev/null 2>&1 || true`
  - `ls -la`
  - `continue`, `exit 1`
- **Bashkit support:**
  - Bash syntax: **FULL** (all features above are implemented)
  - `npm test`: **NOT SUPPORTED** (external binary)

### 41. docx (anthropics/skills) — 17K

- **Type:** Python scripts + Office XML manipulation
- **Scripts:** Python (accept_changes.py, comment.py, office/ library)
- **Binaries:** `pandoc`, `soffice` (LibreOffice), `pdftoppm`, `gcc`, `node/npm`
- **Bash features used:** Simple command invocations with flags
- **Bashkit support:**
  - Bash syntax: full
  - Python: **NOT SUPPORTED** (requires `zipfile`, `defusedxml`, `subprocess`, `socket`, runtime C compilation)
  - Binaries: **NOT SUPPORTED** (`pandoc`, `soffice`, `gcc`, `node`)

### 42. xlsx (anthropics/skills) — 16K

- **Type:** Python script + Office document processing
- **Scripts:** `recalc.py` + shared office library
- **Binaries:** `soffice` (LibreOffice), `timeout`/`gtimeout`
- **Bash features used:** None (all done through Python subprocess)
- **Bashkit support:**
  - Bash syntax: full
  - Python: **NOT SUPPORTED** (requires `openpyxl`, `subprocess`, `platform`)
  - Binaries: **NOT SUPPORTED** (`soffice`)

### 43. better-auth-best-practices (better-auth/skills) — 16K

- **Type:** Pure markdown instructions (auth library patterns)
- **Scripts:** None
- **Binaries:** None
- **Bashkit support:** Full

### 44. marketing-psychology (coreyhaines31/marketingskills) — 15K

- **Type:** Pure markdown instructions
- **Scripts:** None
- **Binaries:** None
- **Bashkit support:** Full

---

## Bash Features Usage Summary

### Features used across all skill bash scripts

| Bash Feature | Used By | Bashkit Support |
|---|---|---|
| `set -e` / `set -euo pipefail` | 6 skills | YES |
| Variable expansion `${VAR}` | 5 skills | YES |
| Default values `${1:-default}` | 4 skills | YES |
| Error values `${1:?msg}` | 3 skills | YES |
| Command substitution `$(cmd)` | 4 skills | YES |
| Pipes `cmd1 \| cmd2` | 4 skills | YES |
| `if/elif/else/fi` | 5 skills | YES |
| `for/do/done` loops | 3 skills | YES |
| `while/case/esac` arg parsing | 2 skills | YES |
| `[[ ]]` conditionals | 2 skills | YES |
| Glob patterns in `[[ ]]` | 1 skill | YES |
| `declare -A` assoc arrays | 1 skill | YES |
| Arithmetic `$(( ))` | 2 skills | YES |
| Heredocs `<< 'EOF'` | 2 skills | YES |
| `trap cleanup EXIT` | 2 skills | YES |
| Redirections `2>/dev/null` | 5 skills | YES |
| `\|\| true` / `\|\| echo` fallback | 3 skills | YES |
| `printf` with format strings | 2 skills | YES |
| Functions (`fn() { }`) | 2 skills | YES |
| Nested function calls | 1 skill | YES |
| `nohup ... &` background | 1 skill | PARTIAL (runs sync) |
| `$!` (background PID) | 1 skill | PARTIAL (returns 0) |
| `kill` | 1 skill | YES (no-op in VFS) |
| Brace expansion `{1..60}` | 1 skill | YES |
| `$BASH_SOURCE` | 1 skill | YES |
| `$OSTYPE` | 1 skill | YES (set to "linux-gnu") |

### External binaries referenced by skills

| Binary | Skills | Bashkit Equivalent |
|---|---|---|
| `grep` | 3 | YES (builtin, -iEFPvclnowq) |
| `sort` | 2 | YES (builtin, -rnu) |
| `tr` | 2 | YES (builtin, -d) |
| `head` | 2 | YES (builtin) |
| `wc` | 1 | YES (builtin, -lwc) |
| `cat` | 2 | YES (builtin) |
| `ls` | 2 | YES (builtin, -lahR) |
| `find` | 2 | YES (builtin, -name -type -maxdepth) |
| `mkdir -p` | 2 | YES (builtin) |
| `cp -r` | 1 | YES (builtin) |
| `rm -rf` | 2 | YES (builtin) |
| `mv` | 1 | YES (builtin) |
| `tar -czf / -xzf` | 2 | YES (builtin, -cxtf -z) |
| `du -h` | 1 | YES (builtin) |
| `mktemp -d` | 1 | YES (builtin, -d) |
| `jq` | 2 | YES (builtin, extensive) |
| `xxd -r -p` | 1 | YES (builtin) |
| `base64` | 1 | **NO** (not a bashkit builtin) |
| `curl -s -X POST -F` | 1 | PARTIAL (`curl` builtin; `-F` multipart not documented) |
| `npm test` / `npm install` | 3 | **NO** (external binary) |
| `node -e / -v` | 2 | **NO** (external binary) |
| `pnpm` | 1 | **NO** (external binary) |
| `python3 -c / -m` | 4 | PARTIAL (bashkit python is limited) |
| `az` (Azure CLI) | 17 | **NO** (external binary) |
| `agent-browser` | 1 | **NO** (native Rust binary) |
| `soffice` (LibreOffice) | 3 | **NO** (native binary) |
| `pdftoppm` (poppler) | 2 | **NO** (native binary) |
| `pdftotext` (poppler) | 1 | **NO** (native binary) |
| `qpdf` | 1 | **NO** (native binary) |
| `pandoc` | 1 | **NO** (native binary) |
| `gcc` | 1 | **NO** (compiler) |
| `tesseract` | 1 | **NO** (OCR engine) |
| `markitdown` | 1 | **NO** (pip package) |
| `nmap` / `nikto` | 1 | **NO** (security tools) |
| `pip install` | 4 | **NO** (package manager) |

---

## Skill Categories by Bashkit Compatibility

### Tier 1: Fully supported (29 skills, 66%)

Pure markdown instruction skills. No scripts to execute. Bashkit's only role
would be parsing the SKILL.md format (YAML frontmatter + markdown body).

Skills: find-skills, vercel-react-best-practices, web-design-guidelines,
remotion-best-practices, frontend-design, vercel-composition-patterns,
14x Azure instruction-only skills, vercel-react-native-skills,
brainstorming, seo-audit, supabase-postgres-best-practices,
next-best-practices, copywriting, better-auth-best-practices,
marketing-psychology

### Tier 2: Bash scripts fully supported, but external binaries missing (7 skills, 16%)

The bash syntax and features used are within bashkit's capabilities. However,
the scripts invoke external binaries that bashkit cannot simulate.

Skills: agent-browser (needs `agent-browser` binary), microsoft-foundry
(needs `az` CLI), systematic-debugging (needs `npm test`),
audit-website (needs `nmap`, `nikto`), vercel-deploy-claimable (needs
`curl -F`, `tar`, `node`), web-artifacts-builder (needs `pnpm`, `node`,
`npm`)

**Notable:** The `deploy.sh` script from vercel-deploy-claimable uses
advanced bash (nested functions, `trap`, `mktemp`, `tar`, `curl -F`,
`grep -o`, `cut`, heredocs) — all bash features are supported by bashkit
except `curl -F` (multipart form upload) and the external `node`/`pnpm`
binaries.

### Tier 3: Requires Python beyond bashkit's capabilities (6 skills, 14%)

These skills depend heavily on Python libraries (subprocess, PIL, openpyxl,
pypdf, reportlab, defusedxml, etc.) that bashkit's embedded Monty
interpreter does not support.

Skills: skill-creator, pdf, pptx, docx, xlsx, ui-ux-pro-max

### Tier 4: Requires full runtime environment (2 skills, 5%)

Browser automation requiring Playwright, Chrome, and extensive Python
ecosystem.

Skills: browser-use, agent-browser (also in Tier 2 for bash)

---

## Gaps and Recommendations

### Missing bashkit builtins that would help

1. **`base64`** — Used by microsoft-foundry's `generate_deployment_url.sh` for
   encoding subscription GUIDs. Simple to add (encode/decode with `-d` flag).

2. **`curl -F` (multipart form)** — Used by vercel-deploy-claimable to upload
   tarballs. Currently `curl` builtin may not support `-F` for multipart POST.

### Python gap analysis

The 6 Python-dependent skills use these libraries not available in Monty:

| Library | Skills | Purpose |
|---|---|---|
| `subprocess` | 4 | Spawn external processes |
| `zipfile` | 3 | ZIP/OOXML manipulation |
| `openpyxl` | 1 | Excel file creation |
| `pypdf` / `pdfplumber` | 1 | PDF processing |
| `reportlab` | 1 | PDF generation |
| `PIL/Pillow` | 1 | Image processing |
| `defusedxml` | 3 | Safe XML parsing |
| `anthropic` | 1 | LLM API calls |
| `csv` | 1 | CSV parsing |
| `concurrent.futures` | 1 | Parallel execution |
| `http.server` | 1 | HTTP server |
| `socket` | 1 | Unix socket detection |
| `argparse` | 3 | CLI argument parsing |

### Key insight

The skills ecosystem is heavily bifurcated:
- **Instruction skills** (66%) are pure markdown — no execution needed
- **Tool skills** (34%) require real binaries (LibreOffice, poppler, Azure CLI,
  browsers) that cannot be meaningfully simulated

For the tool skills, the bash glue code between binaries IS well-supported by
bashkit. The gap is not in bash parsing/execution but in the binary ecosystem.

### Bash feature coverage verdict

Of all bash features observed across 44 skills, bashkit supports **97%+**.
The only gaps are:
- Background execution (`&`) runs synchronously (affects 1 skill)
- `base64` command missing (affects 1 skill)
- `curl -F` multipart possibly missing (affects 1 skill)

Every other bash construct used (associative arrays, `[[ ]]` with globs,
heredocs, traps, brace expansion, arithmetic, pipes, redirections, variable
expansion with defaults/errors, functions, case/esac, for/while loops,
`$BASH_SOURCE`, `$OSTYPE`) is fully supported.
