while IFS=, read -r name age city; do
  echo "$name tem $age anos ($city)"
done < pessoas.csv
while read -r line || [ -n "$line" ]; do echo "[$line]"; done < sem_nl_final.txt
count=0
while read -r _; do count=$((count + 1)); done < pessoas.csv
echo "linhas: $count"
