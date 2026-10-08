unset SHLVL
bash -c 'bash -c "echo \$SHLVL"'
bash -c 'true && bash -c "echo \$SHLVL"'
bash -c 'echo $SHLVL; bash -c "bash -c \"echo \\\$SHLVL\"; true"'
bash -c 'x=$(bash -c "echo \$SHLVL"); echo $x; (bash -c "echo \$SHLVL"); { bash -c "echo \$SHLVL"; }'
bash -c 'exec bash -c "echo \$SHLVL"'
bash -c 'bash -c "echo \$SHLVL" | cat'
