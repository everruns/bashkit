# Scripting Tool Eval: openai/gpt-5.4 (scripted)

- **Date**: 2026-03-13T22:07:11Z
- **Mode**: scripted (ScriptedTool)
- **Max turns**: 10
- **Turns**: 10 total (3.3 avg/task)
- **Tool calls**: 7 total (2.3 avg/task)
- **Tool call success**: 6 ok, 1 error (86% success rate)
- **Tokens**: 7378 input, 737 output
- **Tool output**: 2675 bytes raw, 2707 bytes sent
- **Duration**: 14.8s total (4.9s avg/task)

## Summary

**3/3 tasks passed (100%)**

## By Category

| Category | Passed | Total | Rate | Avg Turns | Avg Calls | Raw Output |
|----------|--------|-------|------|-----------|-----------|------------|
| large_output | 3 | 3 | 100% | 3.3 | 2.3 | 2675 bytes |

## Task Details

### [PASS] lo-large-json-array (large_output)

Sum failed USD transactions from a large JSON array of 50 transaction records

- Tools: 1
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 2.3s
- Tokens: 1077 input, 66 output
- Tool output: 6 bytes raw, 6 bytes sent
- Score: 2/2

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:847.5 | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

### [PASS] lo-verbose-logs (large_output)

Extract and count ERROR lines from verbose log output of ~100 lines

- Tools: 1
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 5.0s
- Tokens: 1334 input, 332 output
- Tool output: 820 bytes raw, 820 bytes sent
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
- Turns: 6 | Tool calls: 5 (4 ok, 1 err) | Duration: 7.5s
- Tokens: 4967 input, 339 output
- Tool output: 1849 bytes raw, 1881 bytes sent
- Score: 3/3

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:postgresql://prod:secret@db.internal:5432/maindb | PASS | found |
| stdout_contains:25 | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

