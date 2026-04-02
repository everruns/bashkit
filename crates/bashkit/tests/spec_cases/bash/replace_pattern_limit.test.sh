# Global pattern replacement size limit
# Regression tests for issue #995

### normal_global_replacement_works
# Normal global replacement still works
val="aaa"
echo "${val//a/bb}"
### expect
bbbbbb
### end

### single_replacement_works
# Single replacement unaffected by limit
val="hello world"
echo "${val/world/universe}"
### expect
hello universe
### end

### global_replacement_empty_pattern
# Empty pattern returns original
val="test"
echo "${val//}"
### expect
test
### end
