### cond_string_equal
# [[ string comparison ==
[[ "hello" == "hello" ]] && echo "yes" || echo "no"
### expect
yes
### end

### cond_string_not_equal
# [[ string !=
[[ "hello" != "world" ]] && echo "yes" || echo "no"
### expect
yes
### end

### cond_string_less
# [[ string <
[[ "abc" < "def" ]] && echo "yes" || echo "no"
### expect
yes
### end

### cond_string_greater
# [[ string >
[[ "def" > "abc" ]] && echo "yes" || echo "no"
### expect
yes
### end

### cond_negation
# [[ ! negation
[[ ! "hello" == "world" ]] && echo "yes" || echo "no"
### expect
yes
### end

### cond_and
# [[ && logical and
[[ "a" == "a" && "b" == "b" ]] && echo "yes" || echo "no"
### expect
yes
### end

### cond_or
# [[ || logical or
[[ "a" == "b" || "b" == "b" ]] && echo "yes" || echo "no"
### expect
yes
### end

### cond_z_empty
# [[ -z empty string
[[ -z "" ]] && echo "yes" || echo "no"
### expect
yes
### end

### cond_n_nonempty
# [[ -n non-empty string
[[ -n "hello" ]] && echo "yes" || echo "no"
### expect
yes
### end

### cond_numeric_eq
# [[ numeric -eq
[[ 42 -eq 42 ]] && echo "yes" || echo "no"
### expect
yes
### end

### cond_numeric_lt
# [[ numeric -lt
[[ 5 -lt 10 ]] && echo "yes" || echo "no"
### expect
yes
### end

### cond_regex_match
# [[ =~ basic regex match
[[ "hello123" =~ ^hello[0-9]+$ ]] && echo "yes" || echo "no"
### expect
yes
### end

### cond_regex_no_match
# [[ =~ regex no match
[[ "hello" =~ ^[0-9]+$ ]] && echo "yes" || echo "no"
### expect
no
### end

### cond_regex_capture
# [[ =~ with BASH_REMATCH capture groups
[[ "hello-world" =~ ^([a-z]+)-([a-z]+)$ ]] && echo "${BASH_REMATCH[1]} ${BASH_REMATCH[2]}"
### expect
hello world
### end

### cond_regex_rematch_full
# [[ =~ BASH_REMATCH[0] is full match
[[ "abc123" =~ [0-9]+ ]] && echo "${BASH_REMATCH[0]}"
### expect
123
### end

### cond_variable_expansion
# [[ with variable expansion
VAR="hello"
[[ "$VAR" == "hello" ]] && echo "yes" || echo "no"
### expect
yes
### end

### cond_exit_code_false
# [[ returns exit code 1 on false
[[ "a" == "b" ]]
echo $?
### expect
1
### end

### cond_glob_star
# [[ == ]] with * glob pattern
[[ "hello.txt" == *.txt ]] && echo match || echo no
### expect
match
### end

### cond_glob_star_no_match
# [[ == ]] glob * doesn't match wrong extension
[[ "hello.txt" == *.log ]] && echo match || echo no
### expect
no
### end

### cond_glob_question
# [[ == ]] with ? single-char glob
[[ "abc" == ?b? ]] && echo match || echo no
### expect
match
### end

### cond_glob_prefix
# [[ == ]] with prefix glob
[[ "hello_world" == hello* ]] && echo match || echo no
### expect
match
### end

### cond_glob_suffix
# [[ == ]] with suffix glob
[[ "test_file" == *_file ]] && echo match || echo no
### expect
match
### end

### cond_glob_not_equal
# [[ != ]] with glob pattern
[[ "hello.txt" != *.log ]] && echo diff || echo same
### expect
diff
### end

### cond_or_and_precedence
# && has higher precedence than || in [[: [[ true || false && false ]] => true
[[ -n "a" || -z "b" && -z "c" ]] && echo "correct" || echo "wrong"
### expect
correct
### end

### cond_or_and_precedence_reverse
# && binds tighter: [[ false && false || true ]] => true
[[ -z "a" && -z "b" || -n "c" ]] && echo "correct" || echo "wrong"
### expect
correct
### end

### cond_exact_match_no_glob
# [[ == ]] exact match with no glob chars
[[ "hello" == "hello" ]] && echo yes || echo no
### expect
yes
### end

### cond_bracket_literal_match
# Issue #837: [] should be treated as literal when it's an invalid glob
x="[]"
if [[ "$x" == "[]" ]]; then echo match; else echo "no match"; fi
### expect
match
### end

### cond_single_bracket_literal
# Literal [ comparison
x="["
[[ "$x" == "[" ]] && echo match || echo "no match"
### expect
match
### end

### cond_valid_glob_bracket
# Valid glob [a] should still work as pattern
[[ "a" == [a] ]] && echo match || echo "no match"
### expect
match
### end
