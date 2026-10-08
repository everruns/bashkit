exec > saida.log 2>&1
echo "vai pro log"
ls naoexiste
exec >/dev/tty 2>/dev/null || true
