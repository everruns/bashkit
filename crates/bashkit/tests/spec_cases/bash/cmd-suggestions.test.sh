### cmd_typo_suggestion
### bash_diff: bashkit adds typo suggestions to command-not-found
### exit_code: 127
# Typo in command name gets suggestion
grpe "test"
### expect
### end

### cmd_unavailable_pip
### bash_diff: bashkit hints for unavailable commands
### exit_code: 127
# pip gets helpful hint
pip install foo
### expect
### end

### cmd_unavailable_sudo
### bash_diff: bashkit hints for unavailable commands
### exit_code: 127
# sudo gets helpful hint
sudo ls
### expect
### end

### cmd_unknown_no_suggestion
### exit_code: 127
### bash_diff: bashkit error messages differ
# Completely unknown command
zzzznonexistent
### expect
### end
