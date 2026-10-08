sh -c 'kill; echo "rc=$?"
kill -l | head -3
kill -l 130; kill -l 9 15
kill -l KILL; echo "rc=$?"
kill -L; echo "rc=$?"
kill -9; echo "rc=$?"
kill -s; echo "rc=$?"
kill -n 9 99999; echo "rc=$?"
kill -- 99999; echo "rc=$?"
kill -0 $$; echo "rc=$?"
kill -SIGTERM 99999; echo "rc=$?"
kill -sigterm 99999; echo "rc=$?"
kill -term 99999 99998; echo "rc=$?"
kill 1.5; echo "rc=$?"
kill %3; echo "rc=$?"' 2>&1
sh -c 'kill -l' | tr '\n' ' '; echo
