# Scripting Tool Eval: openai/gpt-4o (scripted)

- **Date**: 2026-03-24T00:36:10Z
- **Mode**: scripted (ScriptedTool)
- **Max turns**: 10
- **Turns**: 16 total (4.0 avg/task)
- **Tool calls**: 13 total (3.2 avg/task)
- **Inner commands**: 13 total (3.2 avg/task; 5 tool, 5 help, 3 discover)
- **Tool call success**: 13 ok, 0 error (100% success rate)
- **Tokens**: 12253 input, 516 output
- **Tool output**: 2447 bytes raw, 2447 bytes sent
- **Duration**: 20.6s total (5.2s avg/task)

## Summary

**3/4 tasks passed (92%)**

## By Category

| Category | Passed | Total | Rate | Avg Turns | Avg Calls | Avg Inner | Raw Output |
|----------|--------|-------|------|-----------|-----------|-----------|------------|
| discovery | 3 | 4 | 92% | 4.0 | 3.2 | 3.2 | 2447 bytes |

## Task Details

### [PASS] disc-find-by-category (discovery)

Discover tools by category and fetch weather forecast

- Tools: 8
- Turns: 4 | Tool calls: 3 (3 ok, 0 err) | Duration: 8.6s
- Inner commands: 3 (1 tool, 1 help, 1 discover)
- Tokens: 3150 input, 193 output
- Tool output: 824 bytes raw, 824 bytes sent
- Score: 4/4

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:New York | PASS | found |
| stdout_contains:Mon | PASS | found |
| stdout_contains:Sunny | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

### [PASS] disc-search-then-use (discovery)

Search for inventory tool and check stock levels

- Tools: 9
- Turns: 4 | Tool calls: 3 (3 ok, 0 err) | Duration: 4.2s
- Inner commands: 3 (1 tool, 1 help, 1 discover)
- Tokens: 2933 input, 99 output
- Tool output: 314 bytes raw, 314 bytes sent
- Score: 3/3

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:142 | PASS | found |
| stdout_contains:SKU-200 | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

### [FAIL] disc-tag-filter (discovery)

Find read-only tools and compose multi-step customer profile query

- Tools: 8
- Turns: 5 | Tool calls: 5 (5 ok, 0 err) | Duration: 3.6s
- Inner commands: 5 (2 tool, 2 help, 1 discover)
- Tokens: 3916 input, 140 output
- Tool output: 664 bytes raw, 664 bytes sent
- Score: 2/3

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:Alice Johnson | PASS | found |
| stdout_contains:223.49 | FAIL | '223.49' not found in any tool output |
| exit_code:0 | PASS | expected 0, got 0 |

### [PASS] disc-help-json-pipe (discovery)

Learn tool parameters via help and create a support ticket

- Tools: 6
- Turns: 3 | Tool calls: 2 (2 ok, 0 err) | Duration: 4.3s
- Inner commands: 2 (1 tool, 1 help, 0 discover)
- Tokens: 2254 input, 84 output
- Tool output: 645 bytes raw, 645 bytes sent
- Score: 2/2

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:TK-5001 | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

