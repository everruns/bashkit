# Scripting Tool Eval: openai/gpt-5.4 (scripted)

- **Date**: 2026-03-14T04:17:35Z
- **Mode**: scripted (ScriptedTool)
- **Max turns**: 10
- **Turns**: 11 total (2.8 avg/task)
- **Tool calls**: 10 total (2.5 avg/task)
- **Tool call success**: 9 ok, 1 error (90% success rate)
- **Tokens**: 14273 input, 1650 output
- **Tool output**: 2433 bytes raw, 2445 bytes sent
- **Duration**: 23.0s total (5.7s avg/task)

## Summary

**2/4 tasks passed (72%)**

## By Category

| Category | Passed | Total | Rate | Avg Turns | Avg Calls | Raw Output |
|----------|--------|-------|------|-----------|-----------|------------|
| many_tools | 2 | 4 | 72% | 2.8 | 2.5 | 2433 bytes |

## Task Details

### [PASS] mt-ecommerce (many_tools)

E-commerce API: look up user, last order, product details, shipping status, and summarize

- Tools: 18
- Turns: 5 | Tool calls: 5 (4 ok, 1 err) | Duration: 7.3s
- Tokens: 6768 input, 449 output
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

### [FAIL] mt-crm-dashboard (many_tools)

CRM system: look up customer, get support tickets, check subscription, generate summary report

- Tools: 16
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 5.6s
- Tokens: 2398 input, 424 output
- Tool output: 188 bytes raw, 188 bytes sent
- Score: 2/8

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:Acme Corp | PASS | found |
| stdout_contains:Sarah Miller | FAIL | 'Sarah Miller' not found in any tool output |
| stdout_contains:Enterprise Plus | FAIL | 'Enterprise Plus' not found in any tool output |
| stdout_contains:active | FAIL | 'active' not found in any tool output |
| stdout_contains:API rate limiting | FAIL | 'API rate limiting' not found in any tool output |
| stdout_contains:Billing discrepancy | FAIL | 'Billing discrepancy' not found in any tool output |
| tool_calls_min:4 | FAIL | expected >= 4, got 1 |
| tool_calls_max:12 | PASS | expected <= 12, got 1 |

### [PASS] mt-analytics (many_tools)

Analytics platform: get daily metrics, compare with previous day, identify anomalies

- Tools: 20
- Turns: 2 | Tool calls: 2 (2 ok, 0 err) | Duration: 4.0s
- Tokens: 2563 input, 324 output
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
- Turns: 2 | Tool calls: 2 (2 ok, 0 err) | Duration: 6.0s
- Tokens: 2544 input, 453 output
- Tool output: 1248 bytes raw, 1248 bytes sent
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

