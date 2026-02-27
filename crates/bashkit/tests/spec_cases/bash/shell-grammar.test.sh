# Shell grammar edge cases
# Inspired by Oils spec/shell-grammar.test.sh
# https://github.com/oilshell/oil/blob/master/spec/shell-grammar.test.sh

### grammar_brace_group_oneline
# Brace group on one line
{ echo one; echo two; }
### expect
one
two
### end

### grammar_subshell_oneline
# Subshell on one line
(echo one; echo two)
### expect
one
two
### end

### grammar_subshell_multiline
# Subshell on multiple lines
(echo one
echo two
echo three
)
### expect
one
two
three
### end

### grammar_for_do_done
# For loop standard form
for name in a b c
do
  echo $name
done
### expect
a
b
c
### end

### grammar_while_empty_lines
# While loop with empty lines in body
i=0
while [ $i -lt 3 ]; do

  echo $i

  i=$((i+1))

done
### expect
0
1
2
### end

### grammar_until_loop
# Until loop
i=0
until [ $i -ge 3 ]; do
  echo $i
  i=$((i+1))
done
### expect
0
1
2
### end

### grammar_if_then_else
# If with then on separate line
if true
then
  echo yes
else
  echo no
fi
### expect
yes
### end

### grammar_if_then_sameline
# If with then on same line
if true; then
  echo yes
else
  echo no
fi
### expect
yes
### end

### grammar_if_oneline
# If on one line
if true; then echo yes; else echo no; fi
### expect
yes
### end

### grammar_if_pipe
# If condition is a pipeline
if echo hello | grep -q hello; then
  echo matched
fi
### expect
matched
### end

### grammar_case_empty
# Empty case
case foo in
esac
echo done
### expect
done
### end

### grammar_case_without_last_dsemi
# Case without trailing ;;
case foo in
  foo) echo matched
esac
### expect
matched
### end

### grammar_case_with_dsemi
# Case with trailing ;;
case foo in
  foo) echo matched
    ;;
esac
### expect
matched
### end

### grammar_case_empty_clauses
# Case with empty clauses
case foo in
  bar)
    ;;
  foo)
    echo matched
    ;;
esac
### expect
matched
### end

### grammar_case_dsemi_sameline
# Case with ;; on same line
case foo in
  foo) echo matched ;;
esac
### expect
matched
### end

### grammar_case_two_patterns
# Case with two patterns
case b in
  a|b)
    echo matched
    ;;
  c)
    echo no
    ;;
esac
### expect
matched
### end

### grammar_case_oneline
# Case all on one line
case foo in foo) echo matched ;; bar) echo no ;; esac
### expect
matched
### end

### grammar_function_def
# Function definition
f() {
  echo hello
}
f
### expect
hello
### end

### grammar_function_keyword
# Function with keyword
function g {
  echo world
}
g
### expect
world
### end

### grammar_nested_if
# Nested if statements
if true; then
  if false; then
    echo no
  else
    echo yes
  fi
fi
### expect
yes
### end

### grammar_semicolons_and_newlines
# Mixed semicolons and newlines
echo a; echo b
echo c
### expect
a
b
c
### end

### grammar_command_with_ampersand
# Background command (& as separator)
echo foreground
### expect
foreground
### end

### grammar_and_or_lists
# AND and OR lists
true && echo and_true
false && echo and_false
false || echo or_false
true || echo or_true
### expect
and_true
or_false
### end
