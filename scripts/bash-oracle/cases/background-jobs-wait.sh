( echo bg1 ) > bg1.txt &
p1=$!
( exit 4 ) &
p2=$!
wait $p1; echo "w1=$?"
wait $p2; echo "w2=$?"
cat bg1.txt
sleep 0.05 & wait
echo "todos"
