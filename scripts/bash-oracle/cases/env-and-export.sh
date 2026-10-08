export VISIVEL=sim
LOCAL_SO=nao
bash -c 'echo "filho vê: ${VISIVEL:-} ${LOCAL_SO:-nada}"'
TEMP=só-no-comando bash -c 'echo "$TEMP"'
echo "depois: ${TEMP:-vazio}"
env -i X=1 bash -c 'echo "env -i: $X ${HOME:-sem home}"'
echo "$HOME $USER $LC_ALL $TZ"
unset VISIVEL; bash -c 'echo "após unset: ${VISIVEL:-sumiu}"'
