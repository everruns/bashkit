# Glob match cap in remove_pattern_glob
# Regression tests for issue #994

### bracket_prefix_removal_works
# Normal bracket pattern prefix removal still works
x="abcdef"
echo "${x#[a]*}"
### expect
bcdef
### end

### bracket_suffix_removal_works
# Normal bracket pattern suffix removal still works
x="abcdef"
echo "${x%[f]}"
### expect
abcde
### end

### bracket_longest_prefix_removal_works
# Longest bracket pattern prefix removal still works
x="aaabbb"
result="${x##[a]*}"
echo "result=${#result}"
### expect
result=0
### end

### normal_prefix_removal_unaffected
# Standard patterns still work correctly
x="hello_world"
echo "${x#hello_}"
### expect
world
### end
