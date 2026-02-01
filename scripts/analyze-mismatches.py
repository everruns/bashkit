#!/usr/bin/env python3
"""
Analyze differential test mismatch logs.

Usage:
    cargo test --test differential -- --nocapture 2>&1 | python scripts/analyze-mismatches.py

This script parses the output of differential tests and categorizes
mismatches by feature area to help prioritize fixes.
"""

import sys
import re
from collections import defaultdict

def categorize_script(script: str) -> str:
    """Categorize a script by its primary feature."""
    if "for " in script or "while " in script:
        return "loops"
    if "if " in script:
        return "conditionals"
    if "$((" in script:
        return "arithmetic"
    if "=" in script and "$" in script:
        return "variables"
    if script.startswith("echo"):
        return "echo"
    return "other"

def parse_mismatch(lines: list[str]) -> dict | None:
    """Parse a mismatch block from log output."""
    if not lines or not lines[0].startswith("MISMATCH:"):
        return None

    mismatch = {
        "type": "mismatch",
        "script": "",
        "bashkit_output": "",
        "bashkit_exit": 0,
        "real_output": "",
        "real_exit": 0,
    }

    for line in lines[1:]:
        line = line.strip()
        if line.startswith("Script:"):
            # Extract quoted string
            match = re.search(r'"([^"]*)"', line)
            if match:
                mismatch["script"] = match.group(1)
        elif line.startswith("BashKit:"):
            match = re.search(r'"([^"]*)" \(exit (\d+)\)', line)
            if match:
                mismatch["bashkit_output"] = match.group(1)
                mismatch["bashkit_exit"] = int(match.group(2))
        elif line.startswith("Real:"):
            match = re.search(r'"([^"]*)" \(exit (\d+)\)', line)
            if match:
                mismatch["real_output"] = match.group(1)
                mismatch["real_exit"] = int(match.group(2))

    return mismatch if mismatch["script"] else None

def analyze_mismatches(input_text: str) -> dict:
    """Analyze all mismatches in the input."""
    results = {
        "by_category": defaultdict(list),
        "exit_code_only": [],
        "output_only": [],
        "both_differ": [],
        "total": 0,
    }

    # Split into mismatch blocks
    current_block = []
    for line in input_text.split("\n"):
        if line.startswith("MISMATCH:") or line.startswith("BASHKIT_FAILED:"):
            if current_block:
                mismatch = parse_mismatch(current_block)
                if mismatch:
                    process_mismatch(mismatch, results)
            current_block = [line]
        elif current_block:
            current_block.append(line)

    # Process last block
    if current_block:
        mismatch = parse_mismatch(current_block)
        if mismatch:
            process_mismatch(mismatch, results)

    return results

def process_mismatch(mismatch: dict, results: dict):
    """Process a single mismatch and categorize it."""
    results["total"] += 1

    script = mismatch["script"]
    category = categorize_script(script)
    results["by_category"][category].append(mismatch)

    # Classify type of difference
    output_differs = mismatch["bashkit_output"] != mismatch["real_output"]
    exit_differs = mismatch["bashkit_exit"] != mismatch["real_exit"]

    if output_differs and exit_differs:
        results["both_differ"].append(mismatch)
    elif exit_differs:
        results["exit_code_only"].append(mismatch)
    elif output_differs:
        results["output_only"].append(mismatch)

def print_report(results: dict):
    """Print analysis report."""
    print("\n" + "=" * 60)
    print("DIFFERENTIAL TEST MISMATCH ANALYSIS")
    print("=" * 60)

    print(f"\nTotal mismatches: {results['total']}")

    if results["total"] == 0:
        print("\nNo mismatches found! BashKit matches real bash perfectly.")
        return

    print("\n--- By Category ---")
    for category, mismatches in sorted(
        results["by_category"].items(),
        key=lambda x: -len(x[1])
    ):
        print(f"  {category}: {len(mismatches)}")

    print("\n--- By Difference Type ---")
    print(f"  Output differs only: {len(results['output_only'])}")
    print(f"  Exit code differs only: {len(results['exit_code_only'])}")
    print(f"  Both differ: {len(results['both_differ'])}")

    # Show top examples per category
    print("\n--- Sample Mismatches by Category ---")
    for category, mismatches in sorted(results["by_category"].items()):
        print(f"\n[{category}]")
        for m in mismatches[:3]:  # Show up to 3 examples
            print(f"  Script: {m['script']}")
            print(f"    BashKit: {repr(m['bashkit_output'])} (exit {m['bashkit_exit']})")
            print(f"    Real:    {repr(m['real_output'])} (exit {m['real_exit']})")

    # Priority recommendations
    print("\n--- Priority Recommendations ---")
    priority = sorted(
        results["by_category"].items(),
        key=lambda x: -len(x[1])
    )
    for i, (category, mismatches) in enumerate(priority[:3], 1):
        print(f"{i}. Fix {category} ({len(mismatches)} issues)")

    print("\n" + "=" * 60)

def main():
    input_text = sys.stdin.read()
    results = analyze_mismatches(input_text)
    print_report(results)

if __name__ == "__main__":
    main()
