printf '%5s|%-5s|%05d|%x|%o|%e\n' ab cd 42 255 8 1234.5
printf '%.2f %.0f %.3s\n' 3.14159 2.5 abcdef
printf '%b\n' 'a\tb' 'c\nd'
printf '%s,' a b c; echo
printf '%d %d\n' 1 2 3
printf '%c%c\n' hello world
printf -v out '%03d' 7; echo "$out"
printf '%s\n' "${undefined:-padrão}"
