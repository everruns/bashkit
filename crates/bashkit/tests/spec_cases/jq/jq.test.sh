### jq_identity
# Identity filter
echo '{"a":1}' | jq '.'
### expect
{"a":1}
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
# Object keys
echo '{"a":1,"b":2}' | jq 'keys'
### expect
["a","b"]
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
# Map operation
echo '[1,2,3]' | jq 'map(. * 2)'
### expect
[2,4,6]
### end

### jq_add
# Add array elements
echo '[1,2,3]' | jq 'add'
### expect
6
### end

### jq_raw_output
### skip: -r flag not implemented
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
# Object construction
echo '{"a":1,"b":2}' | jq '{x:.a,y:.b}'
### expect
{"x":1,"y":2}
### end

### jq_array_construction
# Array construction
echo '{"a":1,"b":2}' | jq '[.a,.b]'
### expect
[1,2]
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
