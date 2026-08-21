mkdir -p /w && cd /w
printf 'alpha 3\nbeta 1\ngamma 2\n' > items.txt
sort -k2 -n items.txt | awk '{print $1}' | tr '\n' ' '; echo
echo '{"a":[1,2,3]}' | jq -c '.a | add'
t0=$(date +%s); sleep 1; t1=$(date +%s); echo "slept $((t1 - t0))s"
