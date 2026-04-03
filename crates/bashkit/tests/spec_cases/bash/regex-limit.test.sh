# Regex size/complexity limit tests

### grep_normal_regex_works
# Normal regex should work fine
echo "hello world" | grep "hello"
### expect
hello world
### end
