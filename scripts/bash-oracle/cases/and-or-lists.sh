true && echo a || echo b
false && echo c || echo d
false || false || echo e
true && false && echo f; echo "rc=$?"
[ 1 -eq 1 ] && { echo g; false; } || echo h
