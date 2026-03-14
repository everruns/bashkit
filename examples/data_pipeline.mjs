#!/usr/bin/env node
/**
 * Data processing pipeline using bashkit.
 *
 * Demonstrates real-world data tasks: CSV processing, JSON transformation,
 * log analysis, and report generation — all in a sandboxed virtual filesystem.
 *
 * Run:
 *   node examples/data_pipeline.mjs
 */

import { Bash } from "@everruns/bashkit";

function assert(condition, msg = "assertion failed") {
  if (!condition) throw new Error(msg);
}

function demoCsvProcessing() {
  console.log("=== CSV Processing ===\n");

  const bash = new Bash();

  // Create sample sales data
  bash.executeSync(`cat > /tmp/sales.csv << 'EOF'
date,product,quantity,price
2024-01-01,Widget A,10,29.99
2024-01-01,Widget B,5,49.99
2024-01-02,Widget A,8,29.99
2024-01-02,Widget C,12,19.99
2024-01-03,Widget B,3,49.99
2024-01-03,Widget A,15,29.99
2024-01-03,Widget C,7,19.99
EOF`);

  // Total quantity per product
  const r1 = bash.executeSync(
    "tail -n +2 /tmp/sales.csv | awk -F, '{sum[$2]+=$3} END {for (p in sum) print p\": \"sum[p]}' | sort"
  );
  console.log("Quantity by product:");
  console.log(r1.stdout);

  // Top selling day by quantity
  const r2 = bash.executeSync(
    "tail -n +2 /tmp/sales.csv | awk -F, '{sum[$1]+=$3} END {for (d in sum) print sum[d]\" \"d}' | sort -rn | head -1"
  );
  console.log(`Top day: ${r2.stdout.trim()}`);

  // Revenue calculation
  const r3 = bash.executeSync(
    "tail -n +2 /tmp/sales.csv | awk -F, '{rev+=$3*$4} END {printf \"$%.2f\\n\", rev}'"
  );
  console.log(`Total revenue: ${r3.stdout.trim()}`);
  console.log();
}

function demoJsonTransformation() {
  console.log("=== JSON Transformation ===\n");

  const bash = new Bash();

  // Create API-like JSON response
  bash.executeSync(`cat > /tmp/api_response.json << 'EOF'
{
  "users": [
    {"id": 1, "name": "Alice", "email": "alice@example.com", "active": true, "role": "admin"},
    {"id": 2, "name": "Bob", "email": "bob@example.com", "active": false, "role": "user"},
    {"id": 3, "name": "Carol", "email": "carol@example.com", "active": true, "role": "user"},
    {"id": 4, "name": "Dave", "email": "dave@example.com", "active": true, "role": "admin"},
    {"id": 5, "name": "Eve", "email": "eve@example.com", "active": false, "role": "user"}
  ]
}
EOF`);

  // Active admins
  const r1 = bash.executeSync(
    'cat /tmp/api_response.json | jq \'[.users[] | select(.active and .role == "admin")] | length\''
  );
  console.log(`Active admins: ${r1.stdout.trim()}`);
  assert(r1.stdout.trim() === "2");

  // Transform to summary
  const r2 = bash.executeSync(`
    cat /tmp/api_response.json | jq '{
      total: (.users | length),
      active: ([.users[] | select(.active)] | length),
      inactive: ([.users[] | select(.active | not)] | length),
      admins: [.users[] | select(.role == "admin") | .name],
      emails: [.users[] | select(.active) | .email]
    }'
  `);
  const summary = JSON.parse(r2.stdout);
  console.log(`Total: ${summary.total}, Active: ${summary.active}, Inactive: ${summary.inactive}`);
  console.log(`Admins: ${summary.admins.join(", ")}`);
  assert(summary.total === 5);
  assert(summary.active === 3);
  console.log();
}

function demoLogAnalysis() {
  console.log("=== Log Analysis ===\n");

  const bash = new Bash();

  // Create sample log data
  bash.executeSync(`cat > /tmp/access.log << 'EOF'
2024-01-15T10:00:01 GET /api/users 200 45ms
2024-01-15T10:00:02 POST /api/users 201 120ms
2024-01-15T10:00:03 GET /api/users/1 200 30ms
2024-01-15T10:00:04 GET /api/health 200 5ms
2024-01-15T10:00:05 POST /api/login 401 15ms
2024-01-15T10:00:06 GET /api/users 200 42ms
2024-01-15T10:00:07 DELETE /api/users/3 403 10ms
2024-01-15T10:00:08 GET /api/products 500 200ms
2024-01-15T10:00:09 GET /api/users 200 38ms
2024-01-15T10:00:10 POST /api/login 200 95ms
EOF`);

  // Request counts by status code
  const r1 = bash.executeSync(
    "awk '{print $4}' /tmp/access.log | sort | uniq -c | sort -rn"
  );
  console.log("Requests by status:");
  console.log(r1.stdout);

  // Error requests (4xx/5xx)
  const r2 = bash.executeSync(
    "awk '$4 >= 400 {print $1, $2, $3, $4}' /tmp/access.log"
  );
  console.log("Errors:");
  console.log(r2.stdout);

  // Average response time
  const r3 = bash.executeSync(
    "awk '{gsub(/ms/,\"\",$5); sum+=$5; n++} END {printf \"%.1fms\\n\", sum/n}' /tmp/access.log"
  );
  console.log(`Avg response time: ${r3.stdout.trim()}`);

  // Most hit endpoints
  const r4 = bash.executeSync(
    "awk '{print $3}' /tmp/access.log | sort | uniq -c | sort -rn | head -3"
  );
  console.log("Top endpoints:");
  console.log(r4.stdout);
}

function demoReportGeneration() {
  console.log("=== Report Generation ===\n");

  const bash = new Bash();

  // Generate a markdown report from data
  const r = bash.executeSync(`
    # Gather data
    echo -e "alice,95\\nbob,82\\ncarol,91\\ndave,78\\neve,88" > /tmp/scores.csv

    # Build report
    cat << 'HEADER'
# Student Report

| Student | Score | Grade |
|---------|-------|-------|
HEADER

    while IFS=, read name score; do
      if [ "$score" -ge 90 ]; then grade="A"
      elif [ "$score" -ge 80 ]; then grade="B"
      elif [ "$score" -ge 70 ]; then grade="C"
      else grade="F"
      fi
      echo "| $name | $score | $grade |"
    done < /tmp/scores.csv

    echo ""
    AVG=$(awk -F, '{sum+=$2; n++} END {printf "%.1f", sum/n}' /tmp/scores.csv)
    TOP=$(sort -t, -k2 -rn /tmp/scores.csv | head -1 | cut -d, -f1)
    echo "**Average:** $AVG"
    echo "**Top student:** $TOP"
  `);

  console.log(r.stdout);
  assert(r.stdout.includes("alice"));
  assert(r.stdout.includes("Average:"));
}

// ============================================================================

function main() {
  console.log("Bashkit — Data Pipeline Examples\n");
  demoCsvProcessing();
  demoJsonTransformation();
  demoLogAnalysis();
  demoReportGeneration();
  console.log("All examples passed.");
}

main();
