### exec_fd_variable_close
# exec {var}>&- should close fd stored in variable
exec 3>/dev/null
myfd=3
exec {myfd}>&-
echo "closed"
### expect
closed
### end

### exec_fd_variable_open
# exec {var}>file should open fd from variable value
myfd=4
exec {myfd}>/dev/null
echo "opened"
### expect
opened
### end
