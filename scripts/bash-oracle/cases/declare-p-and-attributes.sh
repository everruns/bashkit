declare -i n=10
declare -r ro=fixo
declare -x exp=sim
declare -a idx=(a "b c" [5]=f)
declare -A assoc=([chave]=valor)
declare -l lower=MiXeD
declare -u upper=MiXeD
s="texto com 'aspas'"
declare -p n ro exp idx assoc lower upper s
ro=outro
echo "rc=$?"
declare -p naoexiste
echo "rc=$?"
