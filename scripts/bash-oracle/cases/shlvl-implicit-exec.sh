unset SHLVL
bash -c 'bash -c "echo \$SHLVL"'
bash -c 'bash -c "echo \$SHLVL"; true'
bash -c 'true && bash -c "echo \$SHLVL"'
bash -c 'false || bash -c "echo \$SHLVL"'
bash -c 'echo $SHLVL; bash -c "bash -c \"echo \\\$SHLVL\"; true"'
bash -c 'bash -c "echo \$SHLVL" >/dev/null; echo r'
bash -c 'x=$(bash -c "echo \$SHLVL"); echo $x; (bash -c "echo \$SHLVL"); { bash -c "echo \$SHLVL"; }'
bash -c 'exec bash -c "echo \$SHLVL"'
bash -c 'bash -c "echo \$SHLVL" | cat'
sh -c 'bash -c "echo \$SHLVL"'
sh -c 'echo "sh:${SHLVL-unset}"'
stat -c '%A %N' /bin/sh /usr/bin/sh
