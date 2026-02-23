### string_replace_prefix
s="/usr/local/bin"
echo "${s/#\/usr/PREFIX}"
### expect
PREFIX/local/bin
### end

### string_replace_suffix
s="hello.txt"
echo "${s/%.txt/.md}"
### expect
hello.md
### end

### string_default_colon
echo "${UNSET_VAR:-fallback}"
### expect
fallback
### end

### string_default_empty
X=""
echo "${X:-fallback}"
### expect
fallback
### end

### string_error_message
### bash_diff
### exit_code:1
${UNSET_VAR:?"variable not set"}
### expect
### end

### string_use_replacement
X="present"
echo "${X:+replacement}"
### expect
replacement
### end

### string_use_replacement_empty
EMPTY=""
result="${EMPTY:+replacement}"
echo ">${result}<"
### expect
><
### end

### string_length_unicode
X="hello"
echo "${#X}"
### expect
5
### end

### string_nested_expansion
A="world"
B="A"
echo "${!B}"
### expect
world
### end

### string_concatenation
A="hello"
B="world"
echo "${A} ${B}"
### expect
hello world
### end

### string_uppercase_pattern
X="hello world"
echo "${X^^}"
### expect
HELLO WORLD
### end

### string_lowercase_pattern
X="HELLO WORLD"
echo "${X,,}"
### expect
hello world
### end

### var_negative_substring
X="hello world"
echo "${X: -5}"
### expect
world
### end

### var_substring_length
X="hello world"
echo "${X:0:5}"
### expect
hello
### end
