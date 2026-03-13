# Scripting Tool Eval: openai/gpt-5.2 (scripted)

- **Date**: 2026-03-13T22:04:11Z
- **Mode**: scripted (ScriptedTool)
- **Max turns**: 10
- **Turns**: 29 total (7.2 avg/task)
- **Tool calls**: 28 total (7.0 avg/task)
- **Tool call success**: 18 ok, 10 error (64% success rate)
- **Tokens**: 51927 input, 4392 output
- **Tool output**: 3911 bytes raw, 4136 bytes sent
- **Duration**: 68.2s total (17.0s avg/task)

## Summary

**2/4 tasks passed (88%)**

## By Category

| Category | Passed | Total | Rate | Avg Turns | Avg Calls | Raw Output |
|----------|--------|-------|------|-----------|-----------|------------|
| many_tools | 2 | 4 | 88% | 7.2 | 7.0 | 3911 bytes |

## Task Details

### [PASS] mt-ecommerce (many_tools)

E-commerce API: look up user, last order, product details, shipping status, and summarize

- Tools: 18
- Turns: 7 | Tool calls: 6 (5 ok, 1 err) | Duration: 10.3s
- Tokens: 9818 input, 416 output
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
- Turns: 10 | Tool calls: 10 (5 ok, 5 err) | Duration: 18.5s
- Tokens: 19799 input, 1307 output
- Tool output: 1182 bytes raw, 1279 bytes sent
- Score: 8/8

| Check | Result | Detail |
|-------|--------|--------|
| stdout_contains:Acme Corp | PASS | found |
| stdout_contains:Sarah Miller | PASS | found |
| stdout_contains:Enterprise Plus | PASS | found |
| stdout_contains:active | PASS | found |
| stdout_contains:API rate limiting | PASS | found |
| stdout_contains:Billing discrepancy | PASS | found |
| tool_calls_min:4 | PASS | expected >= 4, got 10 |
| tool_calls_max:12 | PASS | expected <= 12, got 10 |

### [FAIL] mt-analytics (many_tools)

Analytics platform: get daily metrics, compare with previous day, identify anomalies

- Tools: 20
- Turns: 10 | Tool calls: 11 (7 ok, 4 err) | Duration: 32.1s
- Tokens: 19895 input, 2205 output
- Tool output: 824 bytes raw, 940 bytes sent
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
| tool_calls_min:3 | PASS | expected >= 3, got 11 |
| tool_calls_max:10 | FAIL | expected <= 10, got 11 |

### [FAIL] mt-devops (many_tools)

DevOps monitoring: check service health, recent deployments, error rates, determine rollback need

- Tools: 15
- Turns: 2 | Tool calls: 1 (1 ok, 0 err) | Duration: 7.3s
- Tokens: 2415 input, 464 output
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

