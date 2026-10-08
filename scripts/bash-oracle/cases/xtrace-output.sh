set -x
a=1
echo "valor $a" | cat
f() { local b=$((a + 1)); echo "$b"; }
f
set +x
echo fim
