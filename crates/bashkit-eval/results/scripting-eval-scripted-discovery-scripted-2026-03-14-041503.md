# Scripting Tool Eval: anthropic/claude-sonnet-4-20250514 (scripted)

- **Date**: 2026-03-14T04:15:03Z
- **Mode**: scripted (ScriptedTool)
- **Max turns**: 10
- **Turns**: 13 total (3.2 avg/task)
- **Tool calls**: 9 total (2.2 avg/task)
- **Tool call success**: 8 ok, 1 error (89% success rate)
- **Tokens**: 15677 input, 1867 output
- **Tool output**: 2283 bytes raw, 2304 bytes sent
- **Duration**: 38.5s total (9.6s avg/task)

## Summary

**4/4 tasks passed (100%)**

## By Category

| Category | Passed | Total | Rate | Avg Turns | Avg Calls | Raw Output |
|----------|--------|-------|------|-----------|-----------|------------|
| discovery | 4 | 4 | 100% | 3.2 | 2.2 | 2283 bytes |

## Task Details

### [PASS] disc-find-by-category (discovery)

Discover tools by category and fetch weather forecast

- Tools: 8
- Turns: 7 | Tool calls: 6 (5 ok, 1 err) | Duration: 20.8s
- Tokens: 9956 input, 1078 output
- Tool output: 1539 bytes raw, 1560 bytes sent
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
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 4.7s
- Tokens: 1761 input, 153 output
- Tool output: 74 bytes raw, 74 bytes sent
- Score: 2/2

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:142 | PASS | found |
| stdout_contains:SKU-200 | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

### [PASS] disc-tag-filter (discovery)

Find read-only tools and compose multi-step customer profile query

- Tools: 8
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 8.6s
- Tokens: 2218 input, 509 output
- Tool output: 662 bytes raw, 662 bytes sent
- Score: 3/3

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:Alice Johnson | PASS | found |
| stdout_contains:223.49 | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

### [PASS] disc-help-json-pipe (discovery)

Learn tool parameters via help and create a support ticket

- Tools: 6
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 4.3s
- Tokens: 1742 input, 127 output
- Tool output: 8 bytes raw, 8 bytes sent
- Score: 2/2

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:TK-5001 | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

