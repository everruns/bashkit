### conditional_regex_changing_captures
re='^id=([0-9]+)(x)?$'
for value in id=1x id=234 missing id=56; do
  [[ $value =~ $re ]]
  printf '%s:%s:%s:%s\n' "$?" "${#BASH_REMATCH[@]}" "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}"
done
### expect
0:3:1:x
0:3:234:
1:0::
0:3:56:
### end

### conditional_regex_switching_patterns
for re in '^(a)(b)$' '^(ab)$' '^(a)(b)$'; do
  [[ ab =~ $re ]]
  printf '%s:%s:%s\n' "${#BASH_REMATCH[@]}" "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}"
done
### expect
3:a:b
2:ab:
3:a:b
### end

### conditional_regex_utf8_capture_boundaries
re='^(é|😀)(.)?$'
for value in éz 😀 é; do
  [[ $value =~ $re ]]
  printf '%s:%s:%s\n' "$?" "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}"
done
### expect
0:é:z
0:😀:
0:é:
### end

### conditional_regex_nocasematch_switches
re='^(hello)$'
for mode in -u -s -u -s; do
  shopt "$mode" nocasematch
  [[ HELLO =~ $re ]]
  printf '%s:%s\n' "$?" "${BASH_REMATCH[1]}"
done
### expect
1:
0:HELLO
1:
0:HELLO
### end

### conditional_regex_invalid_then_valid
for re in '[' '^(ok)$' '[' '^(ok)$'; do
  [[ ok =~ $re ]]
  printf '%s\n' "$?"
done
### expect
2
0
2
0
### end
