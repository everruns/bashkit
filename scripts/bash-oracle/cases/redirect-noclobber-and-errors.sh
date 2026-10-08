echo a > f.txt
set -o noclobber
echo b > f.txt; echo "rc=$?"
echo c >| f.txt; cat f.txt
set +o noclobber
cat < naoexiste.txt; echo "rc=$?"
echo x > dir/; echo "rc=$?"
echo y > /proc/naopode 2>/dev/null; echo "rc=$?"
