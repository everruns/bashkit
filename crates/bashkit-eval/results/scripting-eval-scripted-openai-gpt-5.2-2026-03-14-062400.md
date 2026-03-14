# Scripting Tool Eval: openai/gpt-5.2 (scripted)

- **Date**: 2026-03-14T06:24:00Z
- **Mode**: scripted (ScriptedTool)
- **Max turns**: 10
- **Turns**: 13 total (3.2 avg/task)
- **Tool calls**: 9 total (2.2 avg/task)
- **Tool call success**: 8 ok, 1 error (89% success rate)
- **Tokens**: 8791 input, 638 output
- **Tool output**: 612 bytes raw, 624 bytes sent
- **Duration**: 14.8s total (3.7s avg/task)

## Summary

**1/4 tasks passed (61%)**

## By Category

| Category | Passed | Total | Rate | Avg Turns | Avg Calls | Raw Output |
|----------|--------|-------|------|-----------|-----------|------------|
| discovery | 1 | 4 | 61% | 3.2 | 2.2 | 612 bytes |

## Task Details

### [FAIL] disc-find-by-category (discovery)

Discover tools by category and fetch weather forecast

- Tools: 8
- Turns: 5 | Tool calls: 4 (4 ok, 0 err) | Duration: 5.5s
- Tokens: 3238 input, 244 output
- Tool output: 104 bytes raw, 104 bytes sent
- Score: 1/4

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:New York | FAIL | 'New York' not found in any tool output |
| stdout_contains:Mon | FAIL | 'Mon' not found in any tool output |
| stdout_contains:Sunny | FAIL | 'Sunny' not found in any tool output |
| exit_code:0 | PASS | expected 0, got 0 |

### [FAIL] disc-search-then-use (discovery)

Search for inventory tool and check stock levels

- Tools: 9
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 2.2s
- Tokens: 1188 input, 56 output
- Tool output: 15 bytes raw, 15 bytes sent
- Score: 2/2

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:142 | PASS | found |
| stdout_contains:SKU-200 | FAIL | 'SKU-200' not found in any tool output |
| exit_code:0 | PASS | expected 0, got 0 |

### [PASS] disc-tag-filter (discovery)

Find read-only tools and compose multi-step customer profile query

- Tools: 8
- Turns: 4 | Tool calls: 3 (2 ok, 1 err) | Duration: 5.4s
- Tokens: 3178 input, 273 output
- Tool output: 488 bytes raw, 500 bytes sent
- Score: 3/3

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:Alice Johnson | PASS | found |
| stdout_contains:223.49 | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

### [FAIL] disc-help-json-pipe (discovery)

Learn tool parameters via help and create a support ticket

- Tools: 6
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 1.7s
- Tokens: 1187 input, 65 output
- Tool output: 5 bytes raw, 5 bytes sent
- Score: 1/2

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:TK-5001 | FAIL | 'TK-5001' not found in any tool output |
| exit_code:0 | PASS | expected 0, got 0 |

