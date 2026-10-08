for v in 1 5 10; do
  if [ "$v" -lt 3 ]; then echo "$v pequeno"
  elif test "$v" -le 5; then echo "$v médio"
  else echo "$v grande"; fi
done
[ -f arq.txt ] && echo existe
[ ! -d arq.txt ] && echo "não é dir"
[ -z "" ] && [ -n "x" ] && echo strings-ok
[ "abc" = "abc" ] && [ "a" != "b" ] && echo eq-ok
[ -s arq.txt ] && [ -r arq.txt ] && [ -e link ] && [ -L link ] && echo fs-ok
