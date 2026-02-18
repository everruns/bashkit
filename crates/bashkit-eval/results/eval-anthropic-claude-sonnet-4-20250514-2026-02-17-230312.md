# Eval Report: anthropic/claude-sonnet-4-20250514

- **Date**: 2026-02-17T23:03:12Z
- **Max turns**: 10
- **Turns**: 7 total (2.3 avg/task)
- **Tool calls**: 4 total (1.3 avg/task)
- **Tool call success**: 4 ok, 0 error (100% success rate)
- **Tokens**: 4468 input, 489 output
- **Duration**: 13.6s total (4.5s avg/task)

## Summary

**3/3 tasks passed (100%)**

## By Category

| Category | Passed | Total | Rate |
|----------|--------|-------|------|
| basic | 3 | 3 | 100% |

## Task Details

### [PASS] smoke_echo (basic)

Simple echo test

- Turns: 2 | Tool calls: 1 (1 ok, 0 error) | Duration: 3.1s
- Tokens: 1181 input, 90 output
- Score: 2/2

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:hello world | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

### [PASS] smoke_file_create (basic)

Create a file and verify

- Turns: 2 | Tool calls: 1 (1 ok, 0 error) | Duration: 4.0s
- Tokens: 1222 input, 144 output
- Score: 4/4

| Check | Result | Detail |
|-------|--------|--------|
| file_exists:/tmp/greeting.txt | PASS | exists |
| file_contains:/tmp/greeting.txt:hi there | PASS | found in file |
| stdout_contains:hi there | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

### [PASS] smoke_grep (basic)

Grep a pre-populated file

- Turns: 3 | Tool calls: 2 (2 ok, 0 error) | Duration: 6.4s
- Tokens: 2065 input, 255 output
- Score: 2/2

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:2 | PASS | found |
| exit_code:0 | PASS | expected 0, got 0 |

