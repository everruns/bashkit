### script_exec_absolute_path
# Execute script by absolute path
echo '#!/bin/bash
echo hello from script' > /tmp/s.sh
chmod +x /tmp/s.sh
/tmp/s.sh
### expect
hello from script
### end

### script_exec_with_args
# Script receives positional args
echo '#!/bin/bash
echo "arg1=$1 arg2=$2"' > /tmp/args.sh
chmod +x /tmp/args.sh
/tmp/args.sh foo bar
### expect
arg1=foo arg2=bar
### end

### script_exec_dollar_zero
# $0 is set to script path
echo '#!/bin/bash
echo $0' > /tmp/name.sh
chmod +x /tmp/name.sh
/tmp/name.sh
### expect
/tmp/name.sh
### end

### script_exec_dollar_hash
# $# shows argument count
echo '#!/bin/bash
echo $#' > /tmp/cnt.sh
chmod +x /tmp/cnt.sh
/tmp/cnt.sh a b c
### expect
3
### end

### script_exec_exit_code
# Exit code propagates from script
echo '#!/bin/bash
exit 42' > /tmp/ex.sh
chmod +x /tmp/ex.sh
/tmp/ex.sh
echo $?
### expect
42
### end

### script_exec_no_shebang
# Script without shebang still runs
echo 'echo no shebang' > /tmp/ns.sh
chmod +x /tmp/ns.sh
/tmp/ns.sh
### expect
no shebang
### end

### script_exec_missing_file
# Missing file: exit 127
/tmp/nonexistent_script.sh
echo $?
### expect
127
### exit_code: 0
### end

### script_exec_permission_denied
# No execute permission: exit 126
echo 'echo nope' > /tmp/nox.sh
/tmp/nox.sh
echo $?
### expect
126
### exit_code: 0
### end

### script_exec_nested_call
# Script calling another script
echo '#!/bin/bash
echo inner' > /tmp/inner.sh
chmod +x /tmp/inner.sh
echo '#!/bin/bash
echo outer
/tmp/inner.sh' > /tmp/outer.sh
chmod +x /tmp/outer.sh
/tmp/outer.sh
### expect
outer
inner
### end

### script_exec_path_search
# $PATH search finds executable
mkdir -p /usr/local/bin
echo '#!/bin/bash
echo found via path' > /usr/local/bin/myutil
chmod +x /usr/local/bin/myutil
PATH=/usr/local/bin
myutil
### expect
found via path
### end
