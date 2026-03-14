# Scripting Tool Eval: openai/gpt-5.4 (scripted)

- **Date**: 2026-03-13T22:07:24Z
- **Mode**: scripted (ScriptedTool)
- **Max turns**: 10
- **Turns**: 15 total (3.8 avg/task)
- **Tool calls**: 16 total (4.0 avg/task)
- **Tool call success**: 15 ok, 1 error (94% success rate)
- **Tokens**: 21371 input, 1801 output
- **Tool output**: 3575 bytes raw, 3587 bytes sent
- **Duration**: 29.3s total (7.3s avg/task)

## Summary

**2/4 tasks passed (91%)**

## By Category

| Category | Passed | Total | Rate | Avg Turns | Avg Calls | Raw Output |
|----------|--------|-------|------|-----------|-----------|------------|
| many_tools | 2 | 4 | 91% | 3.8 | 4.0 | 3575 bytes |

## Task Details

### [PASS] mt-ecommerce (many_tools)

E-commerce API: look up user, last order, product details, shipping status, and summarize

- Tools: 18
- Turns: 6 | Tool calls: 6 (5 ok, 1 err) | Duration: 7.8s
- Tokens: 8498 input, 461 output
- Tool output: 854 bytes raw, 866 bytes sent
- Score: 7/7

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:Jane Doe | PASS | found |
| stdout_contains:ORD-1001 | PASS | found |
| stdout_contains:Wireless Headphones | PASS | found |
| stdout_contains:39.99 | PASS | found |
| stdout_contains:In Transit | PASS | found |
| tool_calls_min:3 | PASS | expected >= 3, got 6 |
| tool_calls_max:10 | PASS | expected <= 10, got 6 |

### [PASS] mt-crm-dashboard (many_tools)

CRM system: look up customer, get support tickets, check subscription, generate summary report

- Tools: 16
- Turns: 5 | Tool calls: 4 (4 ok, 0 err) | Duration: 10.1s
- Tokens: 7739 input, 573 output
- Tool output: 1326 bytes raw, 1326 bytes sent
- Score: 8/8

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:Acme Corp | PASS | found |
| stdout_contains:Sarah Miller | PASS | found |
| stdout_contains:Enterprise Plus | PASS | found |
| stdout_contains:active | PASS | found |
| stdout_contains:API rate limiting | PASS | found |
| stdout_contains:Billing discrepancy | PASS | found |
| tool_calls_min:4 | PASS | expected >= 4, got 4 |
| tool_calls_max:12 | PASS | expected <= 12, got 4 |

### [FAIL] mt-analytics (many_tools)

Analytics platform: get daily metrics, compare with previous day, identify anomalies

- Tools: 20
- Turns: 2 | Tool calls: 2 (2 ok, 0 err) | Duration: 4.5s
- Tokens: 2563 input, 302 output
- Tool output: 344 bytes raw, 344 bytes sent
- Score: 7/8

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:page_views | PASS | found |
| stdout_contains:45200 | PASS | found |
| stdout_contains:52100 | PASS | found |
| stdout_contains:unique_visitors | PASS | found |
| stdout_contains:12800 | PASS | found |
| stdout_contains:14200 | PASS | found |
| stdout_regex:bounce_rate|conversion_rate | PASS | matched |
| tool_calls_min:3 | FAIL | expected >= 3, got 2 |
| tool_calls_max:10 | PASS | expected <= 10, got 2 |

### [FAIL] mt-devops (many_tools)

DevOps monitoring: check service health, recent deployments, error rates, determine rollback need

- Tools: 15
- Turns: 2 | Tool calls: 4 (4 ok, 0 err) | Duration: 6.9s
- Tokens: 2571 input, 465 output
- Tool output: 1051 bytes raw, 1051 bytes sent
- Score: 4/6

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:degraded | PASS | found |
| stdout_contains:v2.4.1 | PASS | found |
| stdout_contains:3.2 | FAIL | '3.2' not found in any tool output |
| stdout_regex:rollback|Rollback|ROLLBACK | FAIL | pattern 'rollback|Rollback|ROLLBACK' not matched |
| stdout_regex:error.rate|Error.rate|ERROR.RATE | PASS | matched |
| tool_calls_min:3 | PASS | expected >= 3, got 4 |
| tool_calls_max:10 | PASS | expected <= 10, got 4 |

