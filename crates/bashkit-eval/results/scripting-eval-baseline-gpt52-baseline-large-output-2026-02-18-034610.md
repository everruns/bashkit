# Scripting Tool Eval: openai/gpt-5.2 (baseline)

- **Date**: 2026-02-18T03:46:10Z
- **Mode**: baseline (individual tools)
- **Max turns**: 10
- **Turns**: 6 total (2.0 avg/task)
- **Tool calls**: 3 total (1.0 avg/task)
- **Tool call success**: 3 ok, 0 error (100% success rate)
- **Tokens**: 8019 input, 365 output
- **Tool output**: 17830 bytes raw, 17830 bytes sent
- **Duration**: 7.6s total (2.5s avg/task)

## Summary

**2/3 tasks passed (90%)**

## By Category

| Category | Passed | Total | Rate | Avg Turns | Avg Calls | Raw Output |
|----------|--------|-------|------|-----------|-----------|------------|
| large_output | 2 | 3 | 90% | 2.0 | 1.0 | 17830 bytes |

## Task Details

### [FAIL] lo-large-json-array (large_output)

Sum failed USD transactions from a large JSON array of 50 transaction records

- Tools: 1
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 1.8s
- Tokens: 3239 input, 40 output
- Tool output: 6907 bytes raw, 6907 bytes sent
- Score: 1/2

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:847.5 | FAIL | '847.5' not found in any tool output |
| exit_code:0 | PASS | expected 0, got 0 |

### [PASS] lo-verbose-logs (large_output)

Extract and count ERROR lines from verbose log output of ~100 lines

- Tools: 1
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 4.2s
- Tokens: 3449 input, 272 output
- Tool output: 8384 bytes raw, 8384 bytes sent
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
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 1.7s
- Tokens: 1331 input, 53 output
- Tool output: 2539 bytes raw, 2539 bytes sent
- Score: 3/3

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:postgresql://prod:secret@db.internal:5432/maindb | PASS | found |
| stdout_contains:25 | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

