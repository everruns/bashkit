#!/usr/bin/env python3
"""
Convert flamegraph SVGs to text format suitable for LLM reading.

This script finds all criterion flamegraph SVGs, copies them to ./flame/,
and generates corresponding .txt files with a hierarchical text representation.
"""

import re
import shutil
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import TypedDict


class Frame(TypedDict):
    """A single stack frame extracted from a flamegraph SVG."""

    name: str
    samples: int
    percentage: float
    y: float
    width: int


def parse_title(title: str) -> tuple[str, int, float] | None:
    """
    Parse a flamegraph title element.

    Returns (function_name, samples, percentage) or None if parsing fails.
    """
    # Match: "function_name (N samples, X.XX%)"
    match = re.match(r'^(.+?)\s+\((\d+)\s+samples?,\s+([\d.]+)%\)$', title)
    if match:
        return match.group(1), int(match.group(2)), float(match.group(3))
    return None


def extract_frames(svg_path: Path) -> list[Frame]:
    """
    Extract stack frames from a flamegraph SVG.

    Returns a list of Frame dicts with keys: name, samples, percentage, y, width.
    """
    tree = ET.parse(svg_path)
    root = tree.getroot()

    frames: list[Frame] = []

    # Find all g elements (they contain title and rect)
    for g in root.iter('{http://www.w3.org/2000/svg}g'):
        title_elem = g.find('{http://www.w3.org/2000/svg}title')
        rect_elem = g.find('{http://www.w3.org/2000/svg}rect')

        if title_elem is None or rect_elem is None:
            continue

        title_text = title_elem.text
        if not title_text:
            continue

        parsed = parse_title(title_text)
        if not parsed:
            continue

        name, samples, percentage = parsed

        # Get y position to determine depth (higher y = deeper in stack for bottom-up)
        y = float(rect_elem.get('y', 0))
        # Get width from fg:w attribute (sample count for this frame)
        fg_w = int(rect_elem.get('{http://github.com/jonhoo/inferno}w', samples))

        frames.append(
            {
                'name': name,
                'samples': samples,
                'percentage': percentage,
                'y': y,
                'width': fg_w,
            }
        )

    return frames


def frames_to_text(frames: list[Frame], benchmark_name: str) -> str:
    """
    Convert frames to a text representation suitable for LLM reading.

    Produces a hierarchical view sorted by sample count (hottest first).
    """
    if not frames:
        return f'# Flamegraph: {benchmark_name}\n\nNo frames found.\n'

    # Sort by samples descending to show hottest functions first
    sorted_frames = sorted(frames, key=lambda f: f['samples'], reverse=True)

    # Get total samples from the root frame (highest sample count)
    total_samples = sorted_frames[0]['samples'] if sorted_frames else 0

    lines = [
        f'# Flamegraph: {benchmark_name}',
        f'# Total samples: {total_samples}',
        '',
        '## Hot Functions (sorted by sample count)',
        '',
    ]

    # Show top functions with their percentages
    seen: set[str] = set()
    for frame in sorted_frames:
        name = frame['name']
        # Skip duplicates (same function at different stack depths)
        if name in seen:
            continue
        seen.add(name)

        samples = frame['samples']
        pct = frame['percentage']

        lines.append(f'{pct:6.2f}% [{samples:5d}] {name}')

    # Add a section showing unique call paths
    lines.extend(
        [
            '',
            '## Stack Frames (by depth)',
            '',
        ]
    )

    # Group frames by y position (depth) and sort
    by_depth: dict[float, list[Frame]] = {}
    for frame in frames:
        y = frame['y']
        if y not in by_depth:
            by_depth[y] = []
        by_depth[y].append(frame)

    # Sort depths (lower y = higher in stack for standard flamegraphs)
    for y in sorted(by_depth.keys(), reverse=True):
        depth_frames = sorted(by_depth[y], key=lambda f: f['samples'], reverse=True)
        depth_num = int((max(by_depth.keys()) - y) / 16)  # Approximate depth from y
        for frame in depth_frames[:10]:  # Limit per depth level
            indent = '  ' * min(depth_num, 10)
            lines.append(f'{indent}{frame["percentage"]:5.2f}% {frame["name"]}')

    lines.append('')
    return '\n'.join(lines)


def find_flamegraph_svgs(target_dir: Path) -> list[tuple[str, Path]]:
    """
    Find all criterion flamegraph SVGs.

    Returns list of (benchmark_name, svg_path) tuples.
    """
    criterion_dir = target_dir / 'criterion'
    if not criterion_dir.exists():
        return []

    results: list[tuple[str, Path]] = []
    # Pattern: target/criterion/<group>/<variant>/profile/flamegraph.svg
    for svg_path in criterion_dir.glob('**/profile/flamegraph.svg'):
        # Extract benchmark name from path
        parts = svg_path.relative_to(criterion_dir).parts
        if len(parts) >= 3:
            benchmark_name = f'{parts[0]}_{parts[1]}'
            results.append((benchmark_name, svg_path))

    return results


def main():
    project_root = Path(__file__).parent.parent
    target_dir = project_root / 'target'
    flame_dir = project_root / 'flame'

    # Clean and recreate flame directory
    if flame_dir.exists():
        shutil.rmtree(flame_dir)
    flame_dir.mkdir()

    # Find all flamegraph SVGs
    svgs = find_flamegraph_svgs(target_dir)

    if not svgs:
        print('No flamegraph SVGs found in target/criterion/')
        print('Run `make profile` first to generate flamegraphs.')
        return

    for benchmark_name, svg_path in svgs:
        # Copy SVG
        dest_svg = flame_dir / f'{benchmark_name}.svg'
        shutil.copy(svg_path, dest_svg)

        # Generate text version
        frames = extract_frames(svg_path)
        text_content = frames_to_text(frames, benchmark_name)

        dest_txt = flame_dir / f'{benchmark_name}.txt'
        dest_txt.write_text(text_content)

    print(f'\n{len(svgs)} flamegraphs written to {flame_dir.name}/ as SVG and text files.')


if __name__ == '__main__':
    main()
