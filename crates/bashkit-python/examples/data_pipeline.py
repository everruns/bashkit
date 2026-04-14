#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.9"
# dependencies = [
#     "bashkit",
# ]
# ///
"""Data processing pipeline using bashkit.

Demonstrates real-world data tasks: CSV processing with bash pipelines
(sort, uniq, awk), JSON transformation with jq, log analysis, and report
generation — all in a sandboxed virtual filesystem.

Run:
    uv run crates/bashkit-python/examples/data_pipeline.py

uv automatically installs bashkit from PyPI (pre-built wheels, no Rust needed).
"""

from __future__ import annotations

import asyncio

from bashkit import Bash


def demo_csv_processing():
    """Process CSV data with bash pipelines."""
    print("=== CSV Processing ===\n")

    bash = Bash()

    # Create sample sales data in the VFS
    bash.execute_sync("""cat > /tmp/sales.csv << 'EOF'
date,product,quantity,price
2024-01-01,Widget A,10,29.99
2024-01-01,Widget B,5,49.99
2024-01-02,Widget A,8,29.99
2024-01-02,Widget C,12,19.99
2024-01-03,Widget B,3,49.99
2024-01-03,Widget A,15,29.99
2024-01-03,Widget C,7,19.99
EOF""")

    # Total quantity per product using awk + sort
    r = bash.execute_sync(
        "tail -n +2 /tmp/sales.csv | awk -F, '{sum[$2]+=$3} END {for (p in sum) print p\": \"sum[p]}' | sort"
    )
    print("Quantity by product:")
    print(r.stdout)

    # Top selling day by total quantity
    r = bash.execute_sync(
        "tail -n +2 /tmp/sales.csv"
        " | awk -F, '{sum[$1]+=$3} END {for (d in sum) print sum[d]\" \"d}'"
        " | sort -rn | head -1"
    )
    print(f"Top day: {r.stdout.strip()}")

    # Revenue calculation
    r = bash.execute_sync("tail -n +2 /tmp/sales.csv | awk -F, '{rev+=$3*$4} END {printf \"$%.2f\\n\", rev}'")
    print(f"Total revenue: {r.stdout.strip()}")

    # Unique products via sort | uniq
    r = bash.execute_sync("tail -n +2 /tmp/sales.csv | cut -d, -f2 | sort | uniq")
    print(f"Products: {', '.join(r.stdout.strip().split(chr(10)))}")

    print()


def demo_json_transformation():
    """Transform JSON data with jq pipelines."""
    print("=== JSON Transformation ===\n")

    bash = Bash()

    # Create API-like JSON response
    bash.execute_sync("""cat > /tmp/api_response.json << 'EOF'
{
  "users": [
    {"id": 1, "name": "Alice", "email": "alice@example.com", "active": true, "role": "admin"},
    {"id": 2, "name": "Bob", "email": "bob@example.com", "active": false, "role": "user"},
    {"id": 3, "name": "Carol", "email": "carol@example.com", "active": true, "role": "user"},
    {"id": 4, "name": "Dave", "email": "dave@example.com", "active": true, "role": "admin"},
    {"id": 5, "name": "Eve", "email": "eve@example.com", "active": false, "role": "user"}
  ]
}
EOF""")

    # Active admins
    r = bash.execute_sync(
        "cat /tmp/api_response.json | jq '[.users[] | select(.active and .role == \"admin\")] | length'"
    )
    print(f"Active admins: {r.stdout.strip()}")
    assert r.stdout.strip() == "2"

    # Build summary object with jq
    r = bash.execute_sync("""
        cat /tmp/api_response.json | jq '{
          total: (.users | length),
          active: ([.users[] | select(.active)] | length),
          inactive: ([.users[] | select(.active | not)] | length),
          admins: [.users[] | select(.role == "admin") | .name],
          emails: [.users[] | select(.active) | .email]
        }'
    """)
    import json

    summary = json.loads(r.stdout)
    print(f"Total: {summary['total']}, Active: {summary['active']}, Inactive: {summary['inactive']}")
    print(f"Admins: {', '.join(summary['admins'])}")
    assert summary["total"] == 5
    assert summary["active"] == 3

    print()


def demo_log_analysis():
    """Analyze log files with grep, awk, sort, uniq."""
    print("=== Log Analysis ===\n")

    bash = Bash()

    # Create sample access log
    bash.execute_sync("""cat > /tmp/access.log << 'EOF'
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
EOF""")

    # Request counts by status code
    r = bash.execute_sync("awk '{print $4}' /tmp/access.log | sort | uniq -c | sort -rn")
    print("Requests by status:")
    print(r.stdout)

    # Error requests (4xx/5xx)
    r = bash.execute_sync("awk '$4 >= 400 {print $1, $2, $3, $4}' /tmp/access.log")
    print("Errors:")
    print(r.stdout)

    # Average response time
    r = bash.execute_sync('awk \'{gsub(/ms/,"",$5); sum+=$5; n++} END {printf "%.1fms\\n", sum/n}\' /tmp/access.log')
    print(f"Avg response time: {r.stdout.strip()}")

    # Most hit endpoints
    r = bash.execute_sync("awk '{print $3}' /tmp/access.log | sort | uniq -c | sort -rn | head -3")
    print("Top endpoints:")
    print(r.stdout)


def demo_vfs_intermediate_files():
    """Demonstrate VFS for intermediate pipeline files."""
    print("=== VFS Intermediate Files ===\n")

    bash = Bash()

    # Stage 1: Generate raw data
    bash.execute_sync("""
        mkdir -p /pipeline/stage1
        for i in $(seq 1 20); do
            echo "$((RANDOM % 100)),$((RANDOM % 5 + 1))" >> /pipeline/stage1/raw.csv
        done
    """)

    # Stage 2: Filter and sort
    bash.execute_sync("""
        mkdir -p /pipeline/stage2
        sort -t, -k1 -n /pipeline/stage1/raw.csv > /pipeline/stage2/sorted.csv
        awk -F, '$1 >= 50' /pipeline/stage2/sorted.csv > /pipeline/stage2/filtered.csv
    """)

    # Stage 3: Aggregate
    bash.execute_sync("""
        mkdir -p /pipeline/stage3
        awk -F, '{sum+=$1; count+=$2; n++} END {
            printf "records=%d\\ntotal=%d\\navg=%.1f\\nweight=%d\\n", n, sum, sum/n, count
        }' /pipeline/stage2/filtered.csv > /pipeline/stage3/summary.txt
    """)

    # Read final result
    r = bash.execute_sync("cat /pipeline/stage3/summary.txt")
    print("Pipeline summary:")
    print(r.stdout)

    # Verify VFS state — all intermediate files exist
    r = bash.execute_sync("find /pipeline -type f | sort")
    print("VFS files:")
    print(r.stdout)

    # Use FileSystem API directly
    fs = bash.fs()
    assert fs.exists("/pipeline/stage3/summary.txt")
    content = fs.read_file("/pipeline/stage3/summary.txt").decode()
    assert "records=" in content
    print(f"Direct VFS read: {content.splitlines()[0]}")

    print()


async def demo_async_pipeline():
    """Async execution of a multi-step pipeline."""
    print("=== Async Pipeline ===\n")

    bash = Bash()

    # Step 1: Create data asynchronously
    await bash.execute("""
        cat > /tmp/inventory.json << 'EOF'
[
  {"item": "laptop", "qty": 15, "price": 999.99},
  {"item": "mouse", "qty": 200, "price": 24.99},
  {"item": "keyboard", "qty": 85, "price": 74.99},
  {"item": "monitor", "qty": 30, "price": 449.99},
  {"item": "cable", "qty": 500, "price": 9.99}
]
EOF
    """)

    # Step 2: Compute total value per item
    r = await bash.execute("""
        cat /tmp/inventory.json \
          | jq -r '.[] | "\\(.item),\\(.qty),\\(.price),\\(.qty * .price)"' \
          | sort -t, -k4 -rn
    """)
    print("Inventory by total value (desc):")
    print(r.stdout)

    # Step 3: Summary
    r = await bash.execute("""
        cat /tmp/inventory.json | jq '{
          total_items: ([.[].qty] | add),
          total_value: ([.[] | .qty * .price] | add | . * 100 | floor / 100),
          most_expensive: (sort_by(-.price) | first | .item),
          most_stocked: (sort_by(-.qty) | first | .item)
        }'
    """)
    print("Summary:")
    print(r.stdout)
    assert r.success

    print()


def demo_report_generation():
    """Generate a markdown report from data."""
    print("=== Report Generation ===\n")

    bash = Bash()

    r = bash.execute_sync("""
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
    """)
    print(r.stdout)
    assert "alice" in r.stdout
    assert "Average:" in r.stdout


# =============================================================================
# Main
# =============================================================================


def main():
    print("Bashkit — Data Pipeline Examples\n")
    demo_csv_processing()
    demo_json_transformation()
    demo_log_analysis()
    demo_vfs_intermediate_files()
    asyncio.run(demo_async_pipeline())
    demo_report_generation()
    print("All examples passed.")


if __name__ == "__main__":
    main()
