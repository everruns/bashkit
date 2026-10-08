# BashBox process-substitution cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_process_substitution_cat_reads_a_substitution
# cat reads a substitution
cat <(echo hi)
### expect
hi
### end

### bashbox_process_substitution_two_substitutions_are_numbered_down_from_63
# two substitutions are numbered down from 63
echo <(true) <(true) >(true); cat <(echo a) <(echo b)
### expect
/dev/fd/63 /dev/fd/62 /dev/fd/61
a
b
### end

### bashbox_process_substitution_the_number_is_reused_once_the_command_is_done
# the number is reused once the command is done
echo <(true); f(){ echo <(true); }; f
### expect
/dev/fd/63
/dev/fd/63
### end

### bashbox_process_substitution_part_of_a_word
# part of a word
### skip: process substitution is its own word, not joined to adjacent text (x<(true) is two words) (limitations.md, Process substitution row)
echo x<(true)
### expect
x/dev/fd/63
### end

### bashbox_process_substitution_quoted_or_escaped_it_stays_text
# quoted or escaped it stays text
echo "<(true)" '<(x)' \<\(y\)
### expect
<(true) <(x) <(y)
### end

### bashbox_process_substitution_redirected_into_a_loop
# redirected into a loop
while read l; do echo "[$l]"; done < <(printf '1\n2\n')
### expect
[1]
[2]
### end

### bashbox_process_substitution_on_another_fd
# on another fd
cat 3< <(echo fd3) <&3; exec 4< <(echo four); read x <&4; echo $x
### expect
fd3
four
### end

### bashbox_process_substitution_its_status_is_not_the_command_status
# its status is not the command status
cat <(echo a; exit 3); echo $?
### expect
a
0
### end

### bashbox_process_substitution_it_runs_in_a_subshell
# it runs in a subshell
x=1; cat <(x=2; echo $x); echo $x
### expect
2
1
### end

### bashbox_process_substitution_assigned_to_a_variable
# assigned to a variable
### skip: x=<(true) parses as an assignment then a separate process-substitution word (limitations.md, Process substitution row)
x=<(true); echo $x
### expect
/dev/fd/63
### end

### bashbox_process_substitution_sort_and_grep_read_it_as_a_file
# sort and grep read it as a file
sort <(printf 'b\na\n'); grep -c x <(printf 'x\ny\nx\n')
### expect
a
b
2
### end

### bashbox_process_substitution_nested_substitutions
# nested substitutions
cat <(cat <(echo inner)); cat <(echo $(echo cmd))
### expect
inner
cmd
### end

### bashbox_process_substitution_a_writer_feeds_the_reader_afterwards
# a writer feeds the reader afterwards
echo hi > >(tr a-z A-Z)
### expect
HI
### end

### bashbox_process_substitution_tee_into_a_reader
# tee into a reader
echo abc | tee >(rev) >/dev/null
### expect
cba
### end

### bashbox_process_substitution_a_reader_whose_file_was_removed
# a reader whose file was removed
rm -f >(cat) 2>/dev/null; echo done
### expect
done
### end

### bashbox_process_substitution_a_case_word
# a case word
case <(true) in /dev/fd/*) echo y;; esac
### expect
y
### end

### bashbox_process_substitution_errexit_holds_inside
# errexit holds inside
set -e; cat <(false; echo x); echo y
### expect
y
### end

### bashbox_process_substitution_a_function_listing_keeps_it
# a function listing keeps it
f(){ cat <(echo a) > >(cat); }; declare -f f
### expect
f () 
{ 
    cat <(echo a) > >(cat)
}
### end
