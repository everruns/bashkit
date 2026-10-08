command -v echo cd ls
type -t echo ls if f 2>/dev/null; f() { :; }; type -t f
command -v naoexiste; echo "rc=$?"
hash -r; type -P sort
