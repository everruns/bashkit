# `${x/pat/rep}`: the replacement is literal text after quote removal (no
# glob escapes leak into it); `${x///}` and `${x////c}` take `/` as the
# pattern; a quoted expansion in the pattern matches literally.

### patsub_replacement_quote_removal
mkdir -p /tmp/patsub_empty && cd /tmp/patsub_empty
v=xx
echo ${v/x/"?"} ${v/x/\?} ${v/x/\\} ${v/x/'\\'} ${v/x/"*"} ${v/x/a\/b}
echo "${v/x/"?"}" "${v/x/\?}"
### expect
?x ?x \x \\x *x a/bx
?x ?x
### end

### patsub_slash_pattern
x=/_/
echo ${x////c} ${x///}
H=/foo/bar
echo ${H////\\/} ${H//'/'/\\/}
### expect
c_c _
\/foo\/bar \/foo\/bar
### end

### patsub_quoted_backslash_pattern
v='[\f]'
x='\f'
echo ${v/"$x"/_} ${v/$x/_} ${v/\f/_} ${v/\\f/_}
### expect
[_] [\_] [\_] [_]
### end

### pattern_reversed_range_and_escaped_dash
x=fooz
echo ${x//[z-a]} ${x//[a\-z]/Q}
[[ z == [z-a] ]] && echo bad || echo nomatch
### expect
fooz fooQ
nomatch
### end
