# BashBox env-commands cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_env_commands_i_starts_empty
# -i starts empty
env -i X=1; env - Y=2
### expect
X=1
Y=2
### end

### bashbox_env_commands_the_command_keeps_its_options_and_stdin
# the command keeps its options and stdin
echo hi | env -i cat -n
### expect
     1	hi
### end
