set -e
if false; then :; fi; echo "if ok"
false || echo "|| ok"
false && echo nunca; echo "&& não sai"
! true; echo "! não sai"
while false; do :; done; echo "while ok"
f() { false; echo "dentro da função após false (contexto de if)"; }
if f; then echo "f ok"; fi
f || echo "não chega aqui porque f retorna 0"
false | true; echo "pipe sem pipefail ok"
echo fim
