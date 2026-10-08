parse() {
  local OPTIND opt verbose=0 out=""
  while getopts ":vo:h" opt; do
    case $opt in
      v) verbose=1 ;;
      o) out=$OPTARG ;;
      h) echo "uso"; return 0 ;;
      :) echo "faltou arg pra -$OPTARG"; return 2 ;;
      \?) echo "opção inválida -$OPTARG"; return 2 ;;
    esac
  done
  shift $((OPTIND - 1))
  echo "verbose=$verbose out=$out resto=$*"
}
parse -v -o saida.txt a b
parse -vo x
parse -x
parse -o
getopts "a" o -a; echo "sem silêncio: $o"
