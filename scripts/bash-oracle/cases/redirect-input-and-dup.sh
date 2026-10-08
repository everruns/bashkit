wc -l < entrada.txt
sort < entrada.txt > ordenado.txt
cat ordenado.txt
tr a-z A-Z < entrada.txt | head -n 1
cat 0< entrada.txt | tail -n 1
