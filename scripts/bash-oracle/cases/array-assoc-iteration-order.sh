declare -A m
for k in alfa beta gama delta epsilon zeta eta teta; do m[$k]=1; done
echo "${!m[@]}"
