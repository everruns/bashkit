s=$'it\'s a "test"'
echo "${s@Q}"
declare -a arr=(x "y z")
echo "${arr[@]@Q}"
n=5; echo "${n@A}"
declare -i num=3; echo "${num@a}"
t='\t'; echo "${t@E}|"
