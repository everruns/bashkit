echo "a$(echo "b$(echo "c$(echo d)")")"
x=$(echo $(echo $(echo deep)))
echo "$x"
y=`echo \`echo crase\``
echo "$y"
z=$(case foo in foo) echo casou;; esac)
echo "$z"
echo "$(echo "aspas \"dentro\"")"
