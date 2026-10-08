for f in a.txt b.TXT img.png Makefile x "com espaço"; do
  case "$f" in
    *.txt|*.TXT) echo "$f: texto" ;;
    *.png) echo "$f: imagem" ;;
    [A-Z]*) echo "$f: maiúscula" ;;
    *" "*) echo "$f: espaço" ;;
    *) echo "$f: outro" ;;
  esac
done
case x in x) echo um ;& y) echo fallthrough ;; z) echo nunca ;; esac
case ab in a*) echo primeiro ;;& *b) echo segundo ;;& *) echo terceiro ;; esac
