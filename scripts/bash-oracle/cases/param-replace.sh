s="foo bar foo baz foo"
echo "${s/foo/X}|${s//foo/X}|${s/#foo/X}|${s/%foo/X}|${s//o}|${s// /_}"
p="a.b.c"; echo "${p//./\/}"
v="CamelCaseName"; echo "${v//[A-Z]/_}"
