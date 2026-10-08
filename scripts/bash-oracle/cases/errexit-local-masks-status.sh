set -e
f() { local x=$(false); echo "local mascarou: x=[$x]"; local y; y=$(false); echo nunca; }
f
echo "rc fora=$?"
