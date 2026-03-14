# Scripting Tool Eval: openai/gpt-5.4 (scripted)

- **Date**: 2026-03-14T04:17:25Z
- **Mode**: scripted (ScriptedTool)
- **Max turns**: 10
- **Turns**: 7 total (2.3 avg/task)
- **Tool calls**: 4 total (1.3 avg/task)
- **Tool call success**: 3 ok, 1 error (75% success rate)
- **Tokens**: 5242 input, 526 output
- **Tool output**: 4719 bytes raw, 4740 bytes sent
- **Duration**: 10.9s total (3.6s avg/task)

## Summary

**3/3 tasks passed (100%)**

## By Category

| Category | Passed | Total | Rate | Avg Turns | Avg Calls | Raw Output |
|----------|--------|-------|------|-----------|-----------|------------|
| large_output | 3 | 3 | 100% | 2.3 | 1.3 | 4719 bytes |

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
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 4.9s
- Tokens: 1334 input, 337 output
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
- Turns: 3 | Tool calls: 2 (1 ok, 1 err) | Duration: 3.7s
- Tokens: 2831 input, 123 output
- Tool output: 3893 bytes raw, 3914 bytes sent
- Score: 3/3

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:postgresql://prod:secret@db.internal:5432/maindb | PASS | found |
| stdout_contains:25 | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

