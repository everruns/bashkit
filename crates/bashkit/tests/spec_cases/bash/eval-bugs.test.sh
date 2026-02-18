### tr_class_upper_from_pipe
### skip: eval-surfaced bug — tr '[:lower:]' '[:upper:]' produces empty output from pipe input
# Bug: echo "hello world" | tr '[:lower:]' '[:upper:]' should produce HELLO WORLD
# Affected eval tasks: script_function_lib (fails all 4 models)
# Root cause: tr POSIX character class translation ([:lower:], [:upper:]) not implemented;
#   pipe input compounds the issue producing empty output instead of passthrough
echo "hello world" | tr '[:lower:]' '[:upper:]'
### expect
HELLO WORLD
### end

### while_read_pipe_vars
### skip: eval-surfaced bug — variables empty inside while-read loop fed by pipe
# Bug: printf "a\nb\n" | while read line; do echo "$line"; done — $line is empty
# Affected eval tasks: complex_markdown_toc (fails all 4 models)
# Root cause: pipe creates subshell for while-read; variable propagation from
#   stdin into the read builtin doesn't work correctly in pipe context
printf "line1\nline2\nline3\n" | while read line; do
  echo "got: $line"
done
### expect
got: line1
got: line2
got: line3
### end

### tail_plus_n_offset
### skip: eval-surfaced bug — tail -n +N returns wrong content (only last lines instead of from line N)
# Bug: tail -n +2 should skip first line and return all remaining lines
# Affected eval tasks: complex_markdown_toc, text_csv_revenue
# Root cause: tail interprets +N as "last N" instead of "starting from line N"
printf 'header\nline1\nline2\nline3\n' | tail -n +2
### expect
line1
line2
line3
### end

### script_chmod_exec_by_path
### skip: eval-surfaced bug — executing script via chmod +x then /path/script.sh fails
# Bug: after chmod +x, running script by absolute path gives "command not found"
# Workaround: bash /path/script.sh works, but direct execution doesn't
# Affected eval tasks: complex_release_notes
# Root cause: VFS executable lookup doesn't check file's execute permission bit
echo '#!/bin/bash
echo "script ran"' > /tmp/test_exec.sh
chmod +x /tmp/test_exec.sh
/tmp/test_exec.sh
### expect
script ran
### end
