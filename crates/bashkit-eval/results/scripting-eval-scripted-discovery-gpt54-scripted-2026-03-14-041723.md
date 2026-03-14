# Scripting Tool Eval: openai/gpt-5.4 (scripted)

- **Date**: 2026-03-14T04:17:23Z
- **Mode**: scripted (ScriptedTool)
- **Max turns**: 10
- **Turns**: 9 total (2.2 avg/task)
- **Tool calls**: 5 total (1.2 avg/task)
- **Tool call success**: 5 ok, 0 error (100% success rate)
- **Tokens**: 6049 input, 485 output
- **Tool output**: 1844 bytes raw, 1844 bytes sent
- **Duration**: 12.4s total (3.1s avg/task)

## Summary

**4/4 tasks passed (100%)**

## By Category

| Category | Passed | Total | Rate | Avg Turns | Avg Calls | Raw Output |
|----------|--------|-------|------|-----------|-----------|------------|
| discovery | 4 | 4 | 100% | 2.2 | 1.2 | 1844 bytes |

## Task Details

### [PASS] disc-find-by-category (discovery)

Discover tools by category and fetch weather forecast

- Tools: 8
- Turns: 3 | Tool calls: 2 (2 ok, 0 err) | Duration: 4.9s
- Tokens: 2015 input, 161 output
- Tool output: 514 bytes raw, 514 bytes sent
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
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 1.7s
- Tokens: 1258 input, 51 output
- Tool output: 295 bytes raw, 295 bytes sent
- Score: 2/2

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:142 | PASS | found |
| stdout_contains:SKU-200 | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

### [PASS] disc-tag-filter (discovery)

Find read-only tools and compose multi-step customer profile query

- Tools: 8
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 3.6s
- Tokens: 1390 input, 182 output
- Tool output: 391 bytes raw, 391 bytes sent
- Score: 3/3

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:Alice Johnson | PASS | found |
| stdout_contains:223.49 | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

### [PASS] disc-help-json-pipe (discovery)

Learn tool parameters via help and create a support ticket

- Tools: 6
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 2.2s
- Tokens: 1386 input, 91 output
- Tool output: 644 bytes raw, 644 bytes sent
- Score: 2/2

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:TK-5001 | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

