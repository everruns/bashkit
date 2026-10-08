echo *
shopt -s dotglob; echo *; shopt -u dotglob
shopt -s globstar; echo **/*.rs; shopt -u globstar
shopt -s extglob
echo !(*.rs)
echo @(a|b).txt
echo +([0-9]).log
echo ?([a-z])x
