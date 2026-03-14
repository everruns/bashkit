# Scripting Tool Eval: openai/gpt-5.4 (scripted)

- **Date**: 2026-03-14T17:44:22Z
- **Mode**: scripted (ScriptedTool)
- **Max turns**: 10
- **Turns**: 15 total (3.8 avg/task)
- **Tool calls**: 14 total (3.5 avg/task)
- **Tool call success**: 13 ok, 1 error (93% success rate)
- **Tokens**: 21808 input, 1830 output
- **Tool output**: 4475 bytes raw, 4487 bytes sent
- **Duration**: 37.0s total (9.3s avg/task)

## Summary

**3/4 tasks passed (93%)**

## By Category

| Category | Passed | Total | Rate | Avg Turns | Avg Calls | Raw Output |
|----------|--------|-------|------|-----------|-----------|------------|
| many_tools | 3 | 4 | 93% | 3.8 | 3.5 | 4475 bytes |

## Task Details

### [PASS] mt-ecommerce (many_tools)

E-commerce API: look up user, last order, product details, shipping status, and summarize

- Tools: 18
- Turns: 5 | Tool calls: 5 (4 ok, 1 err) | Duration: 12.8s
- Tokens: 6848 input, 469 output
- Tool output: 653 bytes raw, 665 bytes sent
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

### [PASS] mt-crm-dashboard (many_tools)

CRM system: look up customer, get support tickets, check subscription, generate summary report

- Tools: 16
- Turns: 6 | Tool calls: 5 (5 ok, 0 err) | Duration: 11.8s
- Tokens: 9399 input, 593 output
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
| tool_calls_min:4 | PASS | expected >= 4, got 5 |
| tool_calls_max:12 | PASS | expected <= 12, got 5 |

### [PASS] mt-analytics (many_tools)

Analytics platform: get daily metrics, compare with previous day, identify anomalies

- Tools: 20
- Turns: 2 | Tool calls: 2 (2 ok, 0 err) | Duration: 5.7s
- Tokens: 2563 input, 287 output
- Tool output: 344 bytes raw, 344 bytes sent
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
| tool_calls_min:2 | PASS | expected >= 2, got 2 |
| tool_calls_max:10 | PASS | expected <= 10, got 2 |

### [FAIL] mt-devops (many_tools)

DevOps monitoring: check service health, recent deployments, error rates, determine rollback need

- Tools: 15
- Turns: 2 | Tool calls: 2 (2 ok, 0 err) | Duration: 6.8s
- Tokens: 2998 input, 481 output
- Tool output: 2152 bytes raw, 2152 bytes sent
- Score: 4/6

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:degraded | PASS | found |
| stdout_contains:v2.4.1 | PASS | found |
| stdout_regex:3\.2%?|0\.032 | PASS | matched |
| stdout_regex:rollback|Rollback|ROLLBACK|roll back | FAIL | pattern 'rollback|Rollback|ROLLBACK|roll back' not matched |
| stdout_regex:error.rate|Error.rate|ERROR.RATE | PASS | matched |
| tool_calls_min:3 | FAIL | expected >= 3, got 2 |
| tool_calls_max:10 | PASS | expected <= 10, got 2 |

