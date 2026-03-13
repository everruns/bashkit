"""
Complete test expectations using CPython.

Scans test_cases/*.py files for incomplete expectations (e.g., `# Return=` with no value)
and fills them in by running the code through CPython.

Supported incomplete patterns:
- `# Return=`      -> fills with repr() of result
- `# Return.str=`  -> fills with str() of result
- `# Return.type=` -> fills with type name of result
- `# Raise=`       -> fills with exception type and message
"""

import re
from pathlib import Path
from typing import Any


def get_cpython_result(code: str, expect_type: str) -> str:
    """Run code through CPython and return the formatted result."""
    # Wrap code in a function that returns the last expression
    lines = code.strip().split('\n')
    last_idx = len(lines) - 1

    # Find last non-empty line
    while last_idx >= 0 and not lines[last_idx].strip():
        last_idx -= 1

    if last_idx < 0:
        raise ValueError('Empty code')

    # Build wrapped function
    if expect_type == 'Raise':
        # For exceptions, don't add return
        func_body = '\n'.join(f'    {line}' if line.strip() else '' for line in lines[: last_idx + 1])
    else:
        # Add return to last line
        func_body = '\n'.join(f'    {line}' if line.strip() else '' for line in lines[:last_idx])
        if func_body:
            func_body += '\n'
        func_body += f'    return {lines[last_idx]}'

    wrapped = f'def __test__():\n{func_body}\n'

    # Execute and get result
    namespace: dict[str, Any] = {}
    exec(wrapped, namespace)

    try:
        result = namespace['__test__']()
        if expect_type == 'Return':
            return repr(result)
        elif expect_type == 'Return.str':
            return str(result)
        elif expect_type == 'Return.type':
            return type(result).__name__
        elif expect_type == 'Raise':
            raise RuntimeError('Expected exception but code completed normally')
        else:
            raise ValueError(f'Unknown expect_type: {expect_type}')
    except Exception as e:
        if expect_type == 'Raise':
            exc_type = type(e).__name__
            msg = str(e)
            if not msg:
                return f'{exc_type}()'
            elif "'" in msg:
                return f'{exc_type}("{msg}")'
            else:
                return f"{exc_type}('{msg}')"
        else:
            raise


incomplete_re = re.compile(r'^# (Return|Return\.str|Return\.type|Raise)=')


def process_file(filepath: Path, dry_run: bool = False) -> bool:
    """Process a single test file. Returns True if file was updated."""
    content = filepath.read_text()
    lines = content.rstrip('\n').split('\n')

    if not lines:
        return False

    if '# test=monty' in lines:
        # testing only with monty, ignore
        return False

    last_line = lines[-1]

    match = incomplete_re.fullmatch(last_line)
    if not match:
        return False

    expect_type = match.group(1)

    code = '\n'.join(lines[:-1])
    result = get_cpython_result(code, expect_type)

    if result == '':
        # happens for empty strings
        return False

    # Update the file
    new_last_line = f'{last_line}{result}'
    lines[-1] = new_last_line
    new_content = '\n'.join(lines) + '\n'

    if dry_run:
        print(f'  Would updated {filepath.name} to assert {new_last_line!r}')
    else:
        filepath.write_text(new_content)
        print(f'  Updated {filepath.name} to assert {new_last_line!r}')

    return True


def main():
    import argparse

    parser = argparse.ArgumentParser(description='Complete test expectations using CPython')
    parser.add_argument('--dry-run', '-n', action='store_true', help='Show what would be done without making changes')
    parser.add_argument('files', nargs='*', help='Specific files to process (default: all test_cases/*.py)')
    args = parser.parse_args()

    # Find test files
    script_dir = Path(__file__).parent
    test_cases_dir = script_dir.parent / 'test_cases'

    if args.files:
        files = [Path(f) for f in args.files]
    else:
        files = sorted(test_cases_dir.glob('*.py'))

    updated = 0
    for filepath in files:
        if process_file(filepath, dry_run=args.dry_run):
            updated += 1

    action = 'Would update' if args.dry_run else 'Updated'
    print(f'\n{action} {updated} file(s)')


if __name__ == '__main__':
    main()
