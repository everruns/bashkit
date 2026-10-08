echo {a,b,c}.txt
echo pre{1..5}
echo {01..10..3}
echo {a..e} {e..a..2}
echo {1..3}{x,y}
echo {a,{b,c}d}
echo {x}  {a..}  "{1..3}"
mkdir -p proj/{src,tests}/{core,util} && find proj -type d | sort
