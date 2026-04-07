# command -v and command -V tests
# Tests that command -v searches PATH for external scripts (issue #1120)

### command_v_finds_builtin
# command -v finds builtins
command -v echo
echo "exit=$?"
### expect
echo
exit=0
### end

### command_v_finds_function
# command -v finds functions
myfunc() { true; }
command -v myfunc
echo "exit=$?"
### expect
myfunc
exit=0
### end

### command_v_not_found
# command -v returns 1 for unknown commands
command -v nonexistent_cmd_xyz_12345
echo "exit=$?"
### expect
exit=1
### end

### command_v_searches_path
# command -v finds executable scripts on PATH
mkdir -p /scripts
echo '#!/bin/bash' > /scripts/myscript
chmod +x /scripts/myscript
export PATH="/scripts:$PATH"
command -v myscript
echo "exit=$?"
### expect
/scripts/myscript
exit=0
### end

### command_V_builtin
# command -V describes builtins
command -V echo
### expect
echo is a shell builtin
### end

### command_V_path_script
# command -V shows full path for scripts on PATH
mkdir -p /scripts
echo '#!/bin/bash' > /scripts/pathcmd
chmod +x /scripts/pathcmd
export PATH="/scripts:$PATH"
command -V pathcmd
### expect
pathcmd is /scripts/pathcmd
### end
