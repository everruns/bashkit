sh -c 'shift; echo "rc=$?"' 2>&1; echo "exit=$?"
sh -c 'shift x; echo "rc=$?"' 2>&1; echo "exit=$?"
sh -c 'set a b; shift 2; echo "rc=$? $#"' 2>&1; echo "exit=$?"
sh -c 'f(){ shift 3; echo in; }; f a; echo "rc=$?"' 2>&1; echo "exit=$?"
