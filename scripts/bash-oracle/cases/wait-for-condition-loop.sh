tries=0
until [ -f pronto.flag ] || [ $tries -ge 3 ]; do
  tries=$((tries + 1))
  [ $tries -eq 2 ] && touch pronto.flag
done
echo "tentativas=$tries"
