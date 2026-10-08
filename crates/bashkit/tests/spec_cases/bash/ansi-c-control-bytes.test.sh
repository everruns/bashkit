# $'\x1e' and $'\x1f' (ASCII record/unit separators) are data. Scripts use
# SEP=$'\x1f' to build composite associative-array keys.

### ansi_c_unit_separator_kept
printf '%s' $'a\x1fb\x1ec' | od -An -c | tr -s ' '
### expect
 a 037 b 036 c
### end

### ansi_c_separator_in_concatenation
k="a"$'\x1f'"b"; printf '%s' "$k" | od -An -c | tr -s ' '
SEP=$'\x1f'; m="x${SEP}y"; echo "${#m}"
### expect
 a 037 b
3
### end

### ansi_c_separator_composite_assoc_key
declare -A v
section=server; key=host
v["${section}"$'\x1f'"${key}"]=localhost
mk="server"$'\x1f'"host"
echo "${v[$mk]}" "${#v[@]}"
for k in "${!v[@]}"; do printf '%s' "$k" | od -An -c | tr -s ' '; done
### expect
localhost 1
 s e r v e r 037 h o s t
### end

### ansi_c_separator_after_single_quote
x='a'$'\x1f''b'; echo "${#x}"
### expect
3
### end

### ansi_c_hex_bytes_decode_as_utf8
# $'\xc3\xa9' is the UTF-8 text é; a lone \x stays literal
printf '%s|%s\n' $'\xc3\xa9' $'\303\251'; echo $'a\x'
### expect
é|é
a\x
### end
