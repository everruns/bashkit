[[ "a" < "B" ]] && echo "a<B" || echo "a>=B"
[ "a" \< "B" ] && echo "test a<B" || echo "test a>=B"
printf '%s\n' b A a B | sort
printf '%s\n' b A a B | LC_ALL=C sort
