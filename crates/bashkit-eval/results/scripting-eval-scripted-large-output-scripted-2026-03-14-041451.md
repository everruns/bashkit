# Scripting Tool Eval: anthropic/claude-sonnet-4-20250514 (scripted)

- **Date**: 2026-03-14T04:14:51Z
- **Mode**: scripted (ScriptedTool)
- **Max turns**: 10
- **Turns**: 9 total (3.0 avg/task)
- **Tool calls**: 6 total (2.0 avg/task)
- **Tool call success**: 5 ok, 1 error (83% success rate)
- **Tokens**: 11410 input, 1170 output
- **Tool output**: 3576 bytes raw, 3597 bytes sent
- **Duration**: 23.1s total (7.7s avg/task)

## Summary

**3/3 tasks passed (100%)**

## By Category

| Category | Passed | Total | Rate | Avg Turns | Avg Calls | Raw Output |
|----------|--------|-------|------|-----------|-----------|------------|
| large_output | 3 | 3 | 100% | 3.0 | 2.0 | 3576 bytes |

## Task Details

### [PASS] lo-large-json-array (large_output)

Sum failed USD transactions from a large JSON array of 50 transaction records

- Tools: 1
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 3.6s
- Tokens: 1621 input, 137 output
- Tool output: 6 bytes raw, 6 bytes sent
- Score: 2/2

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:847.5 | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

### [PASS] lo-verbose-logs (large_output)

Extract and count ERROR lines from verbose log output of ~100 lines

- Tools: 1
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 8.8s
- Tokens: 1996 input, 601 output
- Tool output: 868 bytes raw, 868 bytes sent
- Score: 5/5

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:7 | PASS | found |
| stdout_contains:Connection refused | PASS | found |
| stdout_contains:OutOfMemoryError | PASS | found |
| stdout_contains:Circuit breaker | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

### [PASS] lo-nested-config (large_output)

Extract specific values from a deeply nested JSON configuration object

- Tools: 1
- Turns: 5 | Tool calls: 4 (3 ok, 1 err) | Duration: 10.7s
- Tokens: 7793 input, 432 output
- Tool output: 2702 bytes raw, 2723 bytes sent
- Score: 3/3

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:postgresql://prod:secret@db.internal:5432/maindb | PASS | found |
| stdout_contains:25 | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

