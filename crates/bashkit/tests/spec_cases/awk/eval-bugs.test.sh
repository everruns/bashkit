### awk_field_multiply_accumulate
### skip: eval-surfaced bug — awk field multiplication with += accumulation returns wrong result
# Bug: awk -F',' '{total += $2 * $3} END {print total}' computes wrong sum
# Expected: 10*5 + 25*3 + 7*12 + 15*8 = 50+75+84+120 = 329
# Affected eval tasks: text_csv_revenue (fails 2/4 models)
# Root cause: compound expression $2 * $3 inside += accumulator evaluates incorrectly;
#   simple += with single field works (see awk_variables test), but multiplication
#   of two fields before accumulation produces wrong intermediate values
printf 'widget,10,5\ngadget,25,3\ndoohickey,7,12\nsprocket,15,8\n' | awk -F',' '{total += $2 * $3} END {print total}'
### expect
329
### end

### awk_match_capture_array
### skip: eval-surfaced bug — awk match() with 3rd argument (capture array) unsupported
# Bug: GNU awk match(string, /regex/, array) stores captures in array — bashkit errors
# Affected eval tasks: complex_release_notes, complex_markdown_toc (fails multiple models)
# Root cause: match() builtin only accepts 2 args (string, regex); 3rd arg for
#   capture group extraction is a gawk extension that isn't implemented
printf 'feat(auth): add OAuth2\n' | awk 'match($0, /^([a-z]+)\(([^)]+)\): (.*)/, arr) {print arr[1], arr[2], arr[3]}'
### expect
feat auth add OAuth2
### end
