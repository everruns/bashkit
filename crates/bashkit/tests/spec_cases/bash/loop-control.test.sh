# break/continue edge cases: in a loop condition, in a subshell (which is
# outside any loop), and with too many arguments (the line is abandoned).

### break_in_while_condition
while break; do echo x; done
echo done
for i in 1 2; do while break; do echo x; done; echo i=$i; done
### expect
done
i=1
i=2
### end

### continue_in_subshell_only_warns
for i in 1 2; do
  ( continue; echo in-sub )
  echo st=$?
done 2>/dev/null
### expect
in-sub
st=0
in-sub
st=0
### end

### continue_too_many_args_abandons_line
( for x in a b; do echo $x; continue 1 2; done; echo after )
echo "status=$?"
### expect
a
status=1
### end
