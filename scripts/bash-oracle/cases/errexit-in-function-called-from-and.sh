set -e
check() { echo "check $1"; [ "$1" = ok ]; echo "check passou $1"; }
check ok
check ruim && echo "não imprime"
echo "continua: -e ignorado dentro de && "
check ruim
echo nunca
