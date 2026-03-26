### compgen_c_finds_path_executables
### bash_diff: VFS PATH executables don't exist in real bash
# compgen -c finds executables from $PATH
mkdir -p /usr/local/bin
echo '#!/bin/bash' > /usr/local/bin/tk-list
chmod +x /usr/local/bin/tk-list
echo '#!/bin/bash' > /usr/local/bin/tk-query
chmod +x /usr/local/bin/tk-query
echo '#!/bin/bash' > /usr/local/bin/other-tool
chmod +x /usr/local/bin/other-tool
PATH="/usr/local/bin"
compgen -c "tk-" | sort
### expect
tk-list
tk-query
### end

### compgen_c_includes_builtins
# compgen -c also returns matching builtins
compgen -c "ech" | head -1
### expect
echo
### end
