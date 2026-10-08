echo ~ ~/x
echo "~" '~'
x=~/sub; echo "$x"
echo a:~/b
cd /tmp && echo ~+ && cd - >/dev/null && echo ~-
