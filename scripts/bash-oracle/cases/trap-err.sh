trap 'echo "ERR na linha $LINENO: $BASH_COMMAND rc=$?"' ERR
false
echo depois
f() { return 2; }
f
true
