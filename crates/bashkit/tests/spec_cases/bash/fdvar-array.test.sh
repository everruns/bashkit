### fd_var_array_subscript_closes
exec {fd}>/tmp/fv.txt
echo one >&$fd
exec {fd}>&-
cat /tmp/fv.txt
coproc UP { echo from_coproc; }
exec {UP[1]}>&-
read -r line <&"${UP[0]}"
echo "[$line]"
### expect
one
[from_coproc]
### end

### fd_var_array_is_not_a_command
coproc C { echo x; }
exec {C[1]}>&- 2>&1
echo "rc=$?"
### expect
rc=0
### end
