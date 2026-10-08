# BashBox line-number cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_line_number_compound_commands_use_their_own_line
# compound commands use their own line
for i in $LINENO; do
  echo $LINENO $i
done
case $LINENO in *) echo c$LINENO;; esac
[[ $LINENO == 6 ]] && echo yes
if (( LINENO == 7 ))
then echo $((LINENO)); fi
### expect
2 1
c4
### end

### bashbox_line_number_a_function_body_keeps_its_definition_lines
# a function body keeps its definition lines
f() {
  echo "in $LINENO"
}

f
echo $LINENO
### expect
in 2
6
### end

### bashbox_line_number_lineno_assignments_do_not_stick_a_local_does_unset_makes_it_
# LINENO assignments do not stick, a local does, unset makes it plain
LINENO=50
echo $LINENO
h() { local LINENO=7; echo $LINENO; }
h
echo $LINENO
unset LINENO; echo "[$LINENO]"
echo "[$LINENO]"
### expect
2
7
5
[]
[]
### end

### bashbox_line_number_an_expansion_error_drops_the_rest_of_the_line_it_ends_on
# an expansion error drops the rest of the line it ends on
for i in 1; do
  echo $((1/0))
done; echo same
echo next
echo a; for i in 1; do echo ${x/}; done; echo same
echo next
{ echo a; echo ${y!}; echo b; }; echo same
if true; then
echo ${x!}; fi; echo same
echo last
### expect
next
a

same
next
a
last
### end

### line_number_shift_after_multiline_abort
# bash keeps counting lines from the failing command after a fatal error
# aborts a multi-line top-level command
exec 2>&1
for i in 1; do
  echo $((1/0))

done; echo skipped
echo $LINENO
{
echo ${y!}
}
echo $LINENO
### expect
bash: line 4: 1/0: division by 0 (error token is "0")
5
bash: line 7: ${y!}: bad substitution
8
### end
