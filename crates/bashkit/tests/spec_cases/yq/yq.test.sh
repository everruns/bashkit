### yq_default_identity_stdin
printf 'name: bashkit\nenabled: true\n' | yq
### expect
enabled: true
name: bashkit
### end

### yq_explicit_stdin_file
printf 'name: bashkit\n' | yq '.name' -
### expect
bashkit
### end

### yq_default_identity_file
printf 'name: bashkit\n' > /tmp/default-identity.yml
yq /tmp/default-identity.yml
### expect
name: bashkit
### end

### yq_nested_selection
printf 'root:\n  items:\n    - name: apple\n      kind: fruit\n    - name: carrot\n      kind: vegetable\n' | yq '.root.items[] | select(.kind == "fruit") | .name'
### expect
apple
### end

### yq_eval_alias
printf 'name: bashkit\n' | yq eval '.name'
### expect
bashkit
### end

### yq_named_filter_with_file
printf 'remove: old\nkeep: yes\n' > /tmp/named-filter.yml
yq 'del(.remove)' /tmp/named-filter.yml
### expect
keep: yes
### end

### yq_map_expression
printf 'values: [1, 2, 3]\n' | yq '.values | map(. * 2)'
### expect
- 2
- 4
- 6
### end

### yq_multidocument
printf '%s\n' '---' 'id: 1' '---' 'id: 2' | yq '.id'
### expect
1
---
2
### end

### yq_json_conversion_compact
printf 'name: bashkit\nitems: [1, 2]\n' | yq -o=json -I=0
### expect
{"items":[1,2],"name":"bashkit"}
### end

### yq_json_input_yaml_output
printf '{"name":"bashkit","nested":{"ok":true}}\n' | yq -p=json '.nested'
### expect
ok: true
### end

### yq_raw_combined_flags
printf 'name: bashkit\n' | yq -rje '.name'
### expect
bashkit
### end

### yq_slurp
printf '%s\n' '---' 'id: 1' '---' 'id: 2' | yq -s 'map(.id)'
### expect
- 1
- 2
### end

### yq_null_input
yq -n '{generated: true}'
### expect
generated: true
### end

### yq_invalid_yaml
printf 'a: [1,\n' | yq '.' 2>&1
### expect
yq: invalid YAML: did not find expected node content at line 2 column 1, while parsing a flow node
### exit_code: 1
### end
