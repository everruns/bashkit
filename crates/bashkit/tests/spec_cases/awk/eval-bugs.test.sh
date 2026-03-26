### awk_field_multiply_accumulate
# Bug: awk -F',' '{total += $2 * $3} END {print total}' computes wrong sum
# Expected: 10*5 + 25*3 + 7*12 + 15*8 = 50+75+84+120 = 329
# Affected eval tasks: text_csv_revenue (fails 2/4 models)
# Root cause: compound expression $2 * $3 inside += accumulator evaluated incorrectly
printf 'widget,10,5\ngadget,25,3\ndoohickey,7,12\nsprocket,15,8\n' | awk -F',' '{total += $2 * $3} END {print total}'
### expect
329
### end

### awk_match_capture_array
# Bug: GNU awk match(string, /regex/, array) stores captures in array — bashkit errors
# Affected eval tasks: complex_release_notes, complex_markdown_toc (fails multiple models)
# Root cause: match() builtin only accepted 2 args; 3rd arg for capture group
#   extraction (gawk extension) is now implemented
printf 'feat(auth): add OAuth2\n' | awk 'match($0, /^([a-z]+)\(([^)]+)\): (.*)/, arr) {print arr[1], arr[2], arr[3]}'
### expect
feat auth add OAuth2
### end

### awk_keyword_prefix_identifier
# Bug #852: identifiers starting with keywords split incorrectly
# e.g. print_sp parsed as keyword "print" + variable "_sp"
echo test | awk 'BEGIN { print_sp=0; print_sp++; print print_sp }'
### expect
1
### end

### awk_keyword_prefix_identifier_printf
# Bug #852: printf_count must not split into printf + _count
echo test | awk 'BEGIN { printf_count=5; print printf_count }'
### expect
5
### end

### awk_keyword_prefix_identifier_delete
# Bug #852: delete_flag must not split into delete + _flag
echo test | awk 'BEGIN { delete_flag=42; print delete_flag }'
### expect
42
### end

### awk_keyword_prefix_identifier_return_val
# Bug #852: return_val must not split into return + _val
echo test | awk 'function f() { return_val=99; return return_val } BEGIN { print f() }'
### expect
99
### end

### awk_keyword_prefix_identifier_if_done
# Bug #852: if_done must not split into if + _done
echo test | awk 'BEGIN { if_done=1; while_running=2; for_each=3; print if_done, while_running, for_each }'
### expect
1 2 3
### end
