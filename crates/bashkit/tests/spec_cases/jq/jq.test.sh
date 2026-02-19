### jq_identity
# Identity filter - pretty-printed output
echo '{"a":1}' | jq '.'
### expect
{
  "a": 1
}
### end

### jq_field
# Field access
echo '{"name":"test"}' | jq '.name'
### expect
"test"
### end

### jq_nested
# Nested field access
echo '{"a":{"b":{"c":1}}}' | jq '.a.b.c'
### expect
1
### end

### jq_array_index
# Array index
echo '[1,2,3]' | jq '.[1]'
### expect
2
### end

### jq_array_all
# All array elements
echo '[1,2,3]' | jq '.[]'
### expect
1
2
3
### end

### jq_keys
# Object keys - pretty-printed array output
echo '{"a":1,"b":2}' | jq 'keys'
### expect
[
  "a",
  "b"
]
### end

### jq_length
# Length of array
echo '[1,2,3,4,5]' | jq 'length'
### expect
5
### end

### jq_length_string
# Length of string
echo '"hello"' | jq 'length'
### expect
5
### end

### jq_select
# Select filter
echo '[1,2,3,4,5]' | jq '.[] | select(. > 3)'
### expect
4
5
### end

### jq_map
# Map operation - pretty-printed array output
echo '[1,2,3]' | jq 'map(. * 2)'
### expect
[
  2,
  4,
  6
]
### end

### jq_filter
# Filter with select and array construction - pretty-printed output
echo '[1,2,3,4,5]' | jq '[.[] | select(. > 2)]'
### expect
[
  3,
  4,
  5
]
### end

### jq_map_alternate
# Map with array construction syntax - pretty-printed output
echo '[1,2,3]' | jq '[.[] * 2]'
### expect
[
  2,
  4,
  6
]
### end

### jq_add
# Add array elements
echo '[1,2,3]' | jq 'add'
### expect
6
### end

### jq_raw_output
# Raw output mode outputs strings without quotes
echo '{"name":"test"}' | jq -r '.name'
### expect
test
### end

### jq_type
# Type check
echo '123' | jq 'type'
### expect
"number"
### end

### jq_null
# Null handling
echo '{"a":null}' | jq '.a'
### expect
null
### end

### jq_boolean
# Boolean values
echo 'true' | jq 'not'
### expect
false
### end

### jq_string_interpolation
# String interpolation
echo '{"name":"world"}' | jq '"hello \(.name)"'
### expect
"hello world"
### end

### jq_object_construction
# Object construction - pretty-printed output
echo '{"a":1,"b":2}' | jq '{x:.a,y:.b}'
### expect
{
  "x": 1,
  "y": 2
}
### end

### jq_array_construction
# Array construction - pretty-printed output
echo '{"a":1,"b":2}' | jq '[.a,.b]'
### expect
[
  1,
  2
]
### end

### jq_pipe
# Pipe operator
echo '{"items":[1,2,3]}' | jq '.items | add'
### expect
6
### end

### jq_first
# First element
echo '[1,2,3]' | jq 'first'
### expect
1
### end

### jq_last
# Last element
echo '[1,2,3]' | jq 'last'
### expect
3
### end

### jq_nested_object
# Nested object - pretty-printed with proper indentation
echo '{"a":{"b":{"c":1}}}' | jq '.'
### expect
{
  "a": {
    "b": {
      "c": 1
    }
  }
}
### end

### jq_compact_output
# Compact output mode outputs JSON without pretty-printing
echo '{"a": 1, "b": 2}' | jq -c '.'
### expect
{"a":1,"b":2}
### end

### jq_sort_keys
# Sort keys mode outputs objects with keys in sorted order
echo '{"z":1,"a":2}' | jq -S '.'
### expect
{
  "a": 2,
  "z": 1
}
### end

### jq_slurp
# Slurp mode reads all inputs into a single array
printf '1\n2\n3\n' | jq -s '.'
### expect
[
  1,
  2,
  3
]
### end

### jq_has
# Has function
echo '{"a":1}' | jq 'has("a")'
### expect
true
### end

### jq_has_missing
# Has function for missing key
echo '{"a":1}' | jq 'has("b")'
### expect
false
### end

### jq_values
# Values function
echo '{"a":1,"b":2}' | jq '[.[] | values]'
### expect
[
  1,
  2
]
### end

### jq_empty
# Empty filter
echo '{}' | jq 'empty'
### expect
### end

### jq_split
# String split
echo '"a,b,c"' | jq 'split(",")'
### expect
[
  "a",
  "b",
  "c"
]
### end

### jq_join
# Array join
echo '["a","b","c"]' | jq 'join(",")'
### expect
"a,b,c"
### end

### jq_min
# Min of array
echo '[3,1,2]' | jq 'min'
### expect
1
### end

### jq_max
# Max of array
echo '[3,1,2]' | jq 'max'
### expect
3
### end

### jq_sort
# Sort array
echo '[3,1,2]' | jq 'sort'
### expect
[
  1,
  2,
  3
]
### end

### jq_reverse
# Reverse array
echo '[1,2,3]' | jq 'reverse'
### expect
[
  3,
  2,
  1
]
### end

### jq_unique
# Unique elements
echo '[1,2,2,3,3,3]' | jq 'unique'
### expect
[
  1,
  2,
  3
]
### end

### jq_flatten
# Flatten nested arrays
echo '[[1,2],[3,[4,5]]]' | jq 'flatten'
### expect
[
  1,
  2,
  3,
  4,
  5
]
### end

### jq_group_by
# Group array elements by key
echo '[{"k":"a","v":1},{"k":"b","v":2},{"k":"a","v":3}]' | jq 'group_by(.k)'
### expect
[
  [
    {
      "k": "a",
      "v": 1
    },
    {
      "k": "a",
      "v": 3
    }
  ],
  [
    {
      "k": "b",
      "v": 2
    }
  ]
]
### end

### jq_contains
# Contains check
echo '[1,2,3]' | jq 'contains([2])'
### expect
true
### end

### jq_inside
# Inside check
echo '[2]' | jq 'inside([1,2,3])'
### expect
true
### end

### jq_startswith
# String starts with
echo '"hello world"' | jq 'startswith("hello")'
### expect
true
### end

### jq_endswith
# String ends with
echo '"hello world"' | jq 'endswith("world")'
### expect
true
### end

### jq_ltrimstr
# Left trim string
echo '"hello world"' | jq 'ltrimstr("hello ")'
### expect
"world"
### end

### jq_rtrimstr
# Right trim string
echo '"hello world"' | jq 'rtrimstr(" world")'
### expect
"hello"
### end

### jq_ascii_downcase
# String to lowercase
echo '"HELLO"' | jq 'ascii_downcase'
### expect
"hello"
### end

### jq_ascii_upcase
# String to uppercase
echo '"hello"' | jq 'ascii_upcase'
### expect
"HELLO"
### end

### jq_tonumber
# String to number
echo '"42"' | jq 'tonumber'
### expect
42
### end

### jq_tostring
# Number to string
echo '42' | jq 'tostring'
### expect
"42"
### end

### jq_floor
# Floor function
echo '3.7' | jq 'floor'
### expect
3
### end

### jq_ceil
# Ceiling function
echo '3.2' | jq 'ceil'
### expect
4
### end

### jq_round
# Round function
echo '3.5' | jq 'round'
### expect
4
### end

### jq_abs
# Absolute value
echo '-5' | jq 'abs'
### expect
5
### end

### jq_range
# Range function generates sequence of numbers
jq -n '[range(3)]'
### expect
[
  0,
  1,
  2
]
### end

### jq_nth
# Nth element
echo '[1,2,3,4,5]' | jq 'nth(2)'
### expect
3
### end

### jq_if_then_else
# Conditional
echo '5' | jq 'if . > 3 then "big" else "small" end'
### expect
"big"
### end

### jq_alternative
### skip: jaq errors on .foo applied to null instead of returning null for //
echo 'null' | jq '.foo // "default"'
### expect
"default"
### end

### jq_try
# Try-catch handles runtime errors gracefully
echo 'null' | jq 'try .foo catch "error"'
### expect
"error"
### end

### jq_recurse
# Recurse descends into nested structures
echo '{"a":{"b":1}}' | jq '[recurse | scalars]'
### expect
[
  1
]
### end

### jq_getpath
# Get value at path
echo '{"a":{"b":1}}' | jq 'getpath(["a","b"])'
### expect
1
### end

### jq_setpath
### skip: setpath not available in jaq standard library
echo '{"a":1}' | jq 'setpath(["b"];2)'
### expect
{"a":1,"b":2}
### end

### jq_del
# Delete a key from an object
echo '{"a":1,"b":2}' | jq 'del(.a)'
### expect
{
  "b": 2
}
### end

### jq_to_entries
# Object to entries
echo '{"a":1,"b":2}' | jq 'to_entries'
### expect
[
  {
    "key": "a",
    "value": 1
  },
  {
    "key": "b",
    "value": 2
  }
]
### end

### jq_from_entries
# Entries to object
echo '[{"key":"a","value":1}]' | jq 'from_entries'
### expect
{
  "a": 1
}
### end

### jq_with_entries
# Transform entries
echo '{"a":1}' | jq 'with_entries(.value += 1)'
### expect
{
  "a": 2
}
### end

### jq_paths
# Get all paths in an object
echo '{"a":{"b":1}}' | jq '[paths]'
### expect
[
  [
    "a"
  ],
  [
    "a",
    "b"
  ]
]
### end

### jq_leaf_paths
### skip: leaf_paths not available in jaq standard library
echo '{"a":{"b":1}}' | jq '[leaf_paths]'
### expect
[["a","b"]]
### end

### jq_any
# Any function
echo '[false,true,false]' | jq 'any'
### expect
true
### end

### jq_all
# All function
echo '[true,true,true]' | jq 'all'
### expect
true
### end

### jq_limit
# Limit number of results
echo '[1,2,3,4,5]' | jq '[limit(3;.[])]'
### expect
[
  1,
  2,
  3
]
### end

### jq_until
# Until loop
echo '1' | jq 'until(. >= 5; . + 1)'
### expect
5
### end

### jq_while
# While loop
echo '1' | jq '[while(. < 5; . + 1)]'
### expect
[
  1,
  2,
  3,
  4
]
### end

### jq_input
### skip: inputs iterator not wired to remaining stdin values
printf '1\n2\n' | jq 'input'
### expect
2
### end

### jq_inputs
### skip: inputs iterator not wired to remaining stdin values
printf '1\n2\n3\n' | jq '[inputs]'
### expect
[2,3]
### end

### jq_debug
# Debug passes value through (stderr output not captured)
echo '1' | jq 'debug'
### expect
1
### end

### jq_env
### skip: shell env vars not propagated to jaq runtime
FOO=bar jq -n 'env.FOO'
### expect
"bar"
### end

### jq_multiple_filters
# Multiple filters with comma
echo '{"a":1,"b":2}' | jq '.a, .b'
### expect
1
2
### end

### jq_recursive_descent
# Recursive descent
echo '{"a":{"b":1},"c":2}' | jq '.. | numbers'
### expect
1
2
### end

### jq_optional_object_identifier
# Optional object access
echo '{}' | jq '.foo?'
### expect
null
### end

### jq_reduce
# Reduce iterates over values with accumulator
echo '[1,2,3]' | jq 'reduce .[] as $x (0; . + $x)'
### expect
6
### end

### jq_foreach
# Foreach outputs intermediate accumulator values
echo '[1,2,3]' | jq '[foreach .[] as $x (0; . + $x)]'
### expect
[
  1,
  3,
  6
]
### end

### jq_walk
# Walk recursively transforms all values
echo '{"a":[1,2]}' | jq 'walk(if type == "number" then . + 1 else . end)'
### expect
{
  "a": [
    2,
    3
  ]
}
### end

### jq_gsub
# Global string substitution
echo '"hello"' | jq 'gsub("l";"x")'
### expect
"hexxo"
### end

### jq_sub
# Single string substitution
echo '"hello"' | jq 'sub("l";"x")'
### expect
"hexlo"
### end

### jq_test
# Test string against regex
echo '"hello"' | jq 'test("ell")'
### expect
true
### end

### jq_match
### skip: jaq omits capture name field (real jq includes "name":null)
echo '"hello"' | jq 'match("e(ll)o")'
### expect
{"offset":1,"length":4,"string":"ello","captures":[{"offset":2,"length":2,"string":"ll","name":null}]}
### end

### jq_scan
### skip: jaq scan requires explicit "g" flag for global match
echo '"hello hello"' | jq '[scan("hel")]'
### expect
["hel","hel"]
### end

### jq_index
# Index function
echo '["a","b","c"]' | jq 'index("b")'
### expect
1
### end

### jq_rindex
# Find last index of element
echo '["a","b","a"]' | jq 'rindex("a")'
### expect
2
### end

### jq_indices
# Find all indices of element
echo '["a","b","a"]' | jq 'indices("a")'
### expect
[
  0,
  2
]
### end

### jq_null_input
# Null input mode allows expressions without stdin input
jq -n '1 + 1'
### expect
2
### end

### jq_exit_status
# Exit status mode sets exit code 1 for null/false output
echo 'null' | jq -e '.'
### exit_code: 1
### expect
null
### end

### jq_tab_indent
# Tab indent mode uses tabs instead of spaces for indentation
echo '{"a":1}' | jq --tab '.'
### expect
{
	"a": 1
}
### end

### jq_join_output
# Join output mode suppresses newlines between outputs
echo '["a","b"]' | jq -j '.[]'
### expect
ab
### end

### jq_version
# Version flag outputs version string
jq --version
### expect
jq-1.7.1
### end

### jq_version_short
# Short version flag outputs version string
jq -V
### expect
jq-1.7.1
### end

### jq_file_input
# jq reads from file arguments when provided
mkdir -p /tmp/jqtest
echo '{"a":1}' > /tmp/jqtest/data.json
jq '.' /tmp/jqtest/data.json
### expect
{
  "a": 1
}
### end

### jq_slurp_files
# jq -s slurps multiple file arguments into array
mkdir -p /tmp/jqtest2
echo '{"x":1}' > /tmp/jqtest2/a.json
echo '{"x":2}' > /tmp/jqtest2/b.json
jq -s '.' /tmp/jqtest2/a.json /tmp/jqtest2/b.json
### expect
[
  {
    "x": 1
  },
  {
    "x": 2
  }
]
### end

### jq_combined_flags_rn
# Combined short flags -rn should work like -r -n
jq -rn '"hello"'
### expect
hello
### end

### jq_combined_flags_sc
# Combined short flags -sc should work like -s -c
printf '1\n2\n3\n' | jq -sc 'add'
### expect
6
### end

### jq_arg_binding
# --arg binds a string variable accessible as $name in filter
jq -n --arg greeting hello '"say \($greeting)"'
### expect
"say hello"
### end

### jq_argjson_binding
# --argjson binds a parsed JSON value accessible as $name in filter
jq -n --argjson count 5 '$count + 1'
### expect
6
### end

### jq_empty_input
# Empty input with no flags produces no output
echo '' | jq '.'
### expect
### end

### jq_ndjson
# Multiple JSON values (NDJSON) are each processed separately
printf '{"a":1}\n{"a":2}\n' | jq '.a'
### expect
1
2
### end

### jq_multiple_arg_bindings
# Multiple --arg flags each bind a separate variable
jq -n --arg x hello --arg y world '"[\($x)] [\($y)]"'
### expect
"[hello] [world]"
### end

### jq_argjson_object
# --argjson with a JSON object value
jq -n --argjson obj '{"a":1}' '$obj.a'
### expect
1
### end

### jq_combined_flags_snr
# Three combined short flags -snr
jq -snr '"hello"'
### expect
hello
### end

### jq_exit_status_false
# Exit status mode sets exit code 1 for false output
echo 'false' | jq -e '.'
### exit_code: 1
### expect
false
### end

### jq_exit_status_truthy
# Exit status mode sets exit code 0 for truthy output
echo '42' | jq -e '.'
### exit_code: 0
### expect
42
### end
