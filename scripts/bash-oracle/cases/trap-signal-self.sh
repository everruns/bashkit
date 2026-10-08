trap 'echo pegou-USR1' USR1
kill -USR1 $$
trap 'echo pegou-TERM; exit 143' TERM
kill -TERM $$
echo nunca
