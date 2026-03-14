# Scripting Tool Eval: openai/gpt-5.4 (scripted)

- **Date**: 2026-03-14T17:44:58Z
- **Mode**: scripted (ScriptedTool)
- **Max turns**: 10
- **Turns**: 9 total (2.2 avg/task)
- **Tool calls**: 6 total (1.5 avg/task)
- **Tool call success**: 6 ok, 0 error (100% success rate)
- **Tokens**: 7151 input, 471 output
- **Tool output**: 2410 bytes raw, 2410 bytes sent
- **Duration**: 11.6s total (2.9s avg/task)

## Summary

**0/4 tasks passed (75%)**

## By Category

| Category | Passed | Total | Rate | Avg Turns | Avg Calls | Raw Output |
|----------|--------|-------|------|-----------|-----------|------------|
| discovery | 0 | 4 | 75% | 2.2 | 1.5 | 2410 bytes |

## Task Details

### [FAIL] disc-find-by-category (discovery)

Discover tools by category and fetch weather forecast

- Tools: 8
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 2.8s
- Tokens: 1495 input, 136 output
- Tool output: 505 bytes raw, 505 bytes sent
- Score: 4/5

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:New York | PASS | found |
| stdout_contains:Mon | PASS | found |
| stdout_contains:Sunny | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |
| tool_calls_min:2 | FAIL | expected >= 2, got 1 |

### [FAIL] disc-search-then-use (discovery)

Search for inventory tool and check stock levels

- Tools: 9
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 1.6s
- Tokens: 1437 input, 69 output
- Tool output: 305 bytes raw, 305 bytes sent
- Score: 3/4

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:142 | PASS | found |
| stdout_contains:SKU-200 | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |
| tool_calls_min:2 | FAIL | expected >= 2, got 1 |

### [FAIL] disc-tag-filter (discovery)

Find read-only tools and compose multi-step customer profile query

- Tools: 8
- Turns: 3 | Tool calls: 3 (3 ok, 0 err) | Duration: 4.0s
- Tokens: 2644 input, 178 output
- Tool output: 956 bytes raw, 956 bytes sent
- Score: 3/4

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:Alice Johnson | PASS | found |
| stdout_contains:223.49 | FAIL | '223.49' not found in any tool output |
| exit_code:0 | PASS | expected 0, got 0 |
| tool_calls_min:3 | PASS | expected >= 3, got 3 |

### [FAIL] disc-help-json-pipe (discovery)

Learn tool parameters via help and create a support ticket

- Tools: 6
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 3.2s
- Tokens: 1575 input, 88 output
- Tool output: 644 bytes raw, 644 bytes sent
- Score: 2/3

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:TK-5001 | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |
| tool_calls_min:2 | FAIL | expected >= 2, got 1 |

