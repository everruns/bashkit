# Scripting Tool Eval: openai/gpt-5.2 (scripted)

- **Date**: 2026-02-18T03:46:50Z
- **Mode**: scripted (ScriptedTool)
- **Max turns**: 10
- **Turns**: 20 total (5.0 avg/task)
- **Tool calls**: 16 total (4.0 avg/task)
- **Tool call success**: 7 ok, 9 error (44% success rate)
- **Tokens**: 31053 input, 4204 output
- **Tool output**: 3886 bytes raw, 4077 bytes sent
- **Duration**: 59.7s total (14.9s avg/task)

## Summary

**2/4 tasks passed (88%)**

## By Category

| Category | Passed | Total | Rate | Avg Turns | Avg Calls | Raw Output |
|----------|--------|-------|------|-----------|-----------|------------|
| many_tools | 2 | 4 | 88% | 5.0 | 4.0 | 3886 bytes |

## Task Details

### [PASS] mt-ecommerce (many_tools)

E-commerce API: look up user, last order, product details, shipping status, and summarize

- Tools: 18
- Turns: 6 | Tool calls: 5 (3 ok, 2 err) | Duration: 11.5s
- Tokens: 8506 input, 635 output
- Tool output: 1110 bytes raw, 1152 bytes sent
- Score: 7/7

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:Jane Doe | PASS | found |
| stdout_contains:ORD-1001 | PASS | found |
| stdout_contains:Wireless Headphones | PASS | found |
| stdout_contains:39.99 | PASS | found |
| stdout_contains:In Transit | PASS | found |
| tool_calls_min:3 | PASS | expected >= 3, got 5 |
| tool_calls_max:10 | PASS | expected <= 10, got 5 |

### [FAIL] mt-crm-dashboard (many_tools)

CRM system: look up customer, get support tickets, check subscription, generate summary report

- Tools: 16
- Turns: 4 | Tool calls: 3 (1 ok, 2 err) | Duration: 15.3s
- Tokens: 6210 input, 1133 output
- Tool output: 1131 bytes raw, 1173 bytes sent
- Score: 7/8

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:Acme Corp | PASS | found |
| stdout_contains:Sarah Miller | PASS | found |
| stdout_contains:Enterprise Plus | PASS | found |
| stdout_contains:active | PASS | found |
| stdout_contains:API rate limiting | PASS | found |
| stdout_contains:Billing discrepancy | PASS | found |
| tool_calls_min:4 | FAIL | expected >= 4, got 3 |
| tool_calls_max:12 | PASS | expected <= 12, got 3 |

### [PASS] mt-analytics (many_tools)

Analytics platform: get daily metrics, compare with previous day, identify anomalies

- Tools: 20
- Turns: 8 | Tool calls: 7 (2 ok, 5 err) | Duration: 25.5s
- Tokens: 14196 input, 1937 output
- Tool output: 594 bytes raw, 701 bytes sent
- Score: 8/8

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:page_views | PASS | found |
| stdout_contains:45200 | PASS | found |
| stdout_contains:52100 | PASS | found |
| stdout_contains:unique_visitors | PASS | found |
| stdout_contains:12800 | PASS | found |
| stdout_contains:14200 | PASS | found |
| stdout_regex:bounce_rate|conversion_rate | PASS | matched |
| tool_calls_min:3 | PASS | expected >= 3, got 7 |
| tool_calls_max:10 | PASS | expected <= 10, got 7 |

### [FAIL] mt-devops (many_tools)

DevOps monitoring: check service health, recent deployments, error rates, determine rollback need

- Tools: 15
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 7.4s
- Tokens: 2141 input, 499 output
- Tool output: 1051 bytes raw, 1051 bytes sent
- Score: 4/6

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:degraded | PASS | found |
| stdout_contains:v2.4.1 | PASS | found |
| stdout_contains:3.2 | FAIL | '3.2' not found in any tool output |
| stdout_regex:rollback|Rollback|ROLLBACK | FAIL | pattern 'rollback|Rollback|ROLLBACK' not matched |
| stdout_regex:error.rate|Error.rate|ERROR.RATE | PASS | matched |
| tool_calls_min:3 | FAIL | expected >= 3, got 1 |
| tool_calls_max:10 | PASS | expected <= 10, got 1 |

