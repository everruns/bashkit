# BashBox script-file cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_script_file_a_missing_file
# a missing file
./nope; echo $?
### expect
127
### end

### bashbox_script_file_a_directory
# a directory
mkdir d; ./d; echo $?
### expect
126
### end

### bashbox_script_file_a_file_without_execute_permission
# a file without execute permission
echo 'echo hi' > s; ./s; echo $?
### expect
126
### end

### bashbox_script_file_a_script_gets_its_arguments_and_0_and_only_exported_variable
# a script gets its arguments and $0, and only exported variables
X=2; export Y=3; printf 'echo "$0 $# $1 [$X][$Y][$FOO]"; exit 3' > s; chmod +x s; FOO=f ./s a b; echo $?
### expect
./s 2 a [][3][f]
3
### end

### bashbox_script_file_the_script_s_changes_stay_in_the_child
# the script's changes stay in the child
printf 'f(){ :; }; x=1; cd /; exit 4' > s; chmod 755 s; ./s; echo $? "[$x]"; type -t f; [ "$PWD" != / ] && echo kept
### expect
4 []
kept
### end

### bashbox_script_file_the_script_reads_the_caller_s_stdin
# the script reads the caller's stdin
printf 'read l; echo got $l' > s; chmod +x s; printf 'a\nb\n' | { ./s; read m; echo m=$m; }
### expect
got a
m=b
### end

### bashbox_script_file_a_script_that_does_not_parse_is_reported_under_its_name
# a script that does not parse is reported under its name
printf 'if then' > s; chmod +x s; ./s; echo $?
### expect
2
### end

### bashbox_script_file_command_runs_a_script_file_too
# command runs a script file too
printf 'echo in' > s; chmod +x s; command ./s
### expect
in
### end

### bashbox_script_file_sourcing_dev_null_does_nothing
# sourcing /dev/null does nothing
source /dev/null; echo $?
### expect
0
### end
