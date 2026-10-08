{ bash -c 'kill -9 $$'; } 2>/dev/null; echo "kill9=$?"
{ bash -c 'kill -TERM $$'; } 2>/dev/null; echo "term=$?"
bash -c 'exit 256'; echo "256=$?"
timeout 0.1 sleep 5; echo "timeout=$?"
