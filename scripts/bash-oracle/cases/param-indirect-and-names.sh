cfg_host=localhost cfg_port=80 other=1
ref=cfg_host; echo "${!ref}"
for v in ${!cfg_@}; do echo "$v=${!v}"; done
a=(x y z); echo "${!a[@]}"
declare -A m=([k1]=v1); echo "${!m[@]}"
