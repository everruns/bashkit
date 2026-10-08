set -e
( false; echo nunca-sub ) || echo "subshell falhou com $?"
x=$(false; echo "cmdsub continua sem inherit_errexit")
echo "x=$x"
shopt -s inherit_errexit
y=$(false; echo nunca) || echo "agora herdou: rc=$?"
echo "y=[$y]"
v=$(exit 3)
echo nunca-atribuicao
