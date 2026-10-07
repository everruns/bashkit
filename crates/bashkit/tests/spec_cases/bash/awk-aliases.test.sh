# gawk, mawk and nawk run the awk builtin, so scripts that call a
# specific implementation by name keep working.

### mawk_runs_awk
echo "a b" | mawk '{print $2}'
### expect
b
### end

### gawk_runs_awk
### bash_diff: gawk is not installed on the reference host
echo "a b" | gawk '{print toupper($1)}'
echo x | gawk '{print gensub(/x/, "y", "g")}'
### expect
A
y
### end

### nawk_runs_awk
### bash_diff: nawk is not installed on the reference host
nawk 'BEGIN{print 1+2}'
### expect
3
### end

### awk_aliases_resolve
### bash_diff: gawk/nawk are not installed on the reference host
for c in gawk mawk nawk; do type -t "$c"; done
### expect
file
file
file
### end
