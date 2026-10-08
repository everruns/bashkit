cat log.txt | grep -v DEBUG | awk '{print $2}' | sort | uniq -c | sort -rn | head -n 3
grep -c ERROR log.txt
grep -o 'id=[0-9]*' log.txt | cut -d= -f2 | paste -sd+ | bc
find . -name '*.txt' -type f | sort | xargs wc -l | tail -n 1
