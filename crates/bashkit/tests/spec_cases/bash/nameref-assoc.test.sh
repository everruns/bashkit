### nameref_assign_key_to_assoc
# Nameref assign key to caller's associative array
add_entry() {
  local -n ref="$1"
  ref["hello"]="world"
  ref["foo"]="bar"
}
declare -A mymap
add_entry mymap
echo "${mymap[hello]}"
echo "${mymap[foo]}"
### expect
world
bar
### end

### nameref_iterate_assoc_keys
# Nameref iterate keys of associative array
show_keys() {
  local -n ref="$1"
  for key in "${!ref[@]}"; do
    echo "${key}=${ref[$key]}"
  done | sort
}
declare -A colors=([red]=ff0000 [green]=00ff00 [blue]=0000ff)
show_keys colors
### expect
blue=0000ff
green=00ff00
red=ff0000
### end

### nameref_append_indexed_array
# Nameref append to caller's indexed array
collect() {
  local -n arr_ref="$1"
  arr_ref+=("alpha")
  arr_ref+=("beta")
  arr_ref+=("gamma")
}
items=()
collect items
echo "${#items[@]}"
echo "${items[0]} ${items[1]} ${items[2]}"
### expect
3
alpha beta gamma
### end

### nameref_two_refs_same_function
# Two namerefs in same function (harness pattern)
collect_from() {
  local dir="$1"
  local -n map_ref="$2"
  local -n order_ref="$3"
  map_ref["key-a"]="${dir}/file-a"
  map_ref["key-b"]="${dir}/file-b"
  order_ref+=("key-a")
  order_ref+=("key-b")
}
declare -A my_map
my_order=()
collect_from "/src" my_map my_order
echo "${my_map[key-a]}"
echo "${my_map[key-b]}"
echo "${my_order[0]} ${my_order[1]}"
### expect
/src/file-a
/src/file-b
key-a key-b
### end

### nameref_assoc_length
# Nameref with associative array length
count_entries() {
  local -n ref="$1"
  echo "${#ref[@]}"
}
declare -A data=([x]=1 [y]=2 [z]=3)
count_entries data
### expect
3
### end

### nameref_assoc_key_enumeration_string
# Nameref key enumeration via ${!ref[@]} in string context
show() {
  local -n ref="$1"
  for k in "${!ref[@]}"; do
    echo "${k}=${ref[$k]}"
  done | sort
}
declare -A m=([a]=1 [b]=2 [c]=3)
show m
### expect
a=1
b=2
c=3
### end

### nameref_overwrite_assoc_key
# Nameref overwrite existing key in associative array
update() {
  local -n ref="$1"
  ref["name"]="updated"
}
declare -A record=([name]=original [age]=30)
update record
echo "${record[name]}"
echo "${record[age]}"
### expect
updated
30
### end
