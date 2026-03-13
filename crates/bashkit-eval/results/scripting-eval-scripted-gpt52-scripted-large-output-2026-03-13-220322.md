# Scripting Tool Eval: openai/gpt-5.2 (scripted)

- **Date**: 2026-03-13T22:03:22Z
- **Mode**: scripted (ScriptedTool)
- **Max turns**: 10
- **Turns**: 10 total (3.3 avg/task)
- **Tool calls**: 7 total (2.3 avg/task)
- **Tool call success**: 5 ok, 2 error (71% success rate)
- **Tokens**: 9069 input, 760 output
- **Tool output**: 4794 bytes raw, 4827 bytes sent
- **Duration**: 17.6s total (5.9s avg/task)

## Summary

**3/3 tasks passed (100%)**

## By Category

| Category | Passed | Total | Rate | Avg Turns | Avg Calls | Raw Output |
|----------|--------|-------|------|-----------|-----------|------------|
| large_output | 3 | 3 | 100% | 3.3 | 2.3 | 4794 bytes |

## Task Details

### [PASS] lo-large-json-array (large_output)

Sum failed USD transactions from a large JSON array of 50 transaction records

- Tools: 1
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 2.3s
- Tokens: 1071 input, 60 output
- Tool output: 6 bytes raw, 6 bytes sent
- Score: 2/2

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:847.5 | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

### [PASS] lo-verbose-logs (large_output)

Extract and count ERROR lines from verbose log output of ~100 lines

- Tools: 1
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 6.2s
- Tokens: 1353 input, 352 output
- Tool output: 838 bytes raw, 838 bytes sent
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
- Turns: 6 | Tool calls: 5 (3 ok, 2 err) | Duration: 9.2s
- Tokens: 6645 input, 348 output
- Tool output: 3950 bytes raw, 3983 bytes sent
- Score: 3/3

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:postgresql://prod:secret@db.internal:5432/maindb | PASS | found |
| stdout_contains:25 | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

