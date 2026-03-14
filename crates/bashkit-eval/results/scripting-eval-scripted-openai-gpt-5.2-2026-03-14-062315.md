# Scripting Tool Eval: openai/gpt-5.2 (scripted)

- **Date**: 2026-03-14T06:23:15Z
- **Mode**: scripted (ScriptedTool)
- **Max turns**: 10
- **Turns**: 25 total (6.2 avg/task)
- **Tool calls**: 22 total (5.5 avg/task)
- **Tool call success**: 15 ok, 7 error (68% success rate)
- **Tokens**: 47792 input, 4703 output
- **Tool output**: 4046 bytes raw, 4177 bytes sent
- **Duration**: 63.7s total (15.9s avg/task)

## Summary

**3/4 tasks passed (93%)**

## By Category

| Category | Passed | Total | Rate | Avg Turns | Avg Calls | Raw Output |
|----------|--------|-------|------|-----------|-----------|------------|
| many_tools | 3 | 4 | 93% | 6.2 | 5.5 | 4046 bytes |

## Task Details

### [PASS] mt-ecommerce (many_tools)

E-commerce API: look up user, last order, product details, shipping status, and summarize

- Tools: 18
- Turns: 7 | Tool calls: 6 (5 ok, 1 err) | Duration: 9.3s
- Tokens: 9824 input, 429 output
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
- Turns: 9 | Tool calls: 8 (6 ok, 2 err) | Duration: 15.5s
- Tokens: 15683 input, 1108 output
- Tool output: 1164 bytes raw, 1188 bytes sent
- Score: 8/8

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:Acme Corp | PASS | found |
| stdout_contains:Sarah Miller | PASS | found |
| stdout_contains:Enterprise Plus | PASS | found |
| stdout_contains:active | PASS | found |
| stdout_contains:API rate limiting | PASS | found |
| stdout_contains:Billing discrepancy | PASS | found |
| tool_calls_min:4 | PASS | expected >= 4, got 8 |
| tool_calls_max:12 | PASS | expected <= 12, got 8 |

### [PASS] mt-analytics (many_tools)

Analytics platform: get daily metrics, compare with previous day, identify anomalies

- Tools: 20
- Turns: 7 | Tool calls: 7 (3 ok, 4 err) | Duration: 32.6s
- Tokens: 19870 input, 2741 output
- Tool output: 977 bytes raw, 1072 bytes sent
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
| tool_calls_min:2 | PASS | expected >= 2, got 7 |
| tool_calls_max:10 | PASS | expected <= 10, got 7 |

### [FAIL] mt-devops (many_tools)

DevOps monitoring: check service health, recent deployments, error rates, determine rollback need

- Tools: 15
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 6.3s
- Tokens: 2415 input, 425 output
- Tool output: 1051 bytes raw, 1051 bytes sent
- Score: 4/6

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:degraded | PASS | found |
| stdout_contains:v2.4.1 | PASS | found |
| stdout_regex:3\.2%?|0\.032 | PASS | matched |
| stdout_regex:rollback|Rollback|ROLLBACK|roll back | FAIL | pattern 'rollback|Rollback|ROLLBACK|roll back' not matched |
| stdout_regex:error.rate|Error.rate|ERROR.RATE | PASS | matched |
| tool_calls_min:3 | FAIL | expected >= 3, got 1 |
| tool_calls_max:10 | PASS | expected <= 10, got 1 |

