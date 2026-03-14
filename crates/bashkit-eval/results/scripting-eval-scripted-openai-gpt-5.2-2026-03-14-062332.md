# Scripting Tool Eval: openai/gpt-5.2 (scripted)

- **Date**: 2026-03-14T06:23:32Z
- **Mode**: scripted (ScriptedTool)
- **Max turns**: 10
- **Turns**: 14 total (4.7 avg/task)
- **Tool calls**: 12 total (4.0 avg/task)
- **Tool call success**: 7 ok, 5 error (58% success rate)
- **Tokens**: 11631 input, 896 output
- **Tool output**: 1530 bytes raw, 1637 bytes sent
- **Duration**: 16.9s total (5.6s avg/task)

## Summary

**2/3 tasks passed (80%)**

## By Category

| Category | Passed | Total | Rate | Avg Turns | Avg Calls | Raw Output |
|----------|--------|-------|------|-----------|-----------|------------|
| large_output | 2 | 3 | 80% | 4.7 | 4.0 | 1530 bytes |

## Task Details

### [PASS] lo-large-json-array (large_output)

Sum failed USD transactions from a large JSON array of 50 transaction records

- Tools: 1
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 1.7s
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
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 4.3s
- Tokens: 1373 input, 373 output
- Tool output: 838 bytes raw, 838 bytes sent
- Score: 5/5

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:7 | PASS | found |
| stdout_contains:Connection refused | PASS | found |
| stdout_contains:OutOfMemoryError | PASS | found |
| stdout_contains:Circuit breaker | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

### [FAIL] lo-nested-config (large_output)

Extract specific values from a deeply nested JSON configuration object

- Tools: 1
- Turns: 10 | Tool calls: 10 (5 ok, 5 err) | Duration: 10.9s
- Tokens: 9187 input, 463 output
- Tool output: 686 bytes raw, 793 bytes sent
- Score: 1/3

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:postgresql://prod:secret@db.internal:5432/maindb | FAIL | 'postgresql://prod:secret@db.internal:5432/maindb' not found in any tool output |
| stdout_contains:25 | FAIL | '25' not found in any tool output |
| exit_code:0 | PASS | expected 0, got 0 |

