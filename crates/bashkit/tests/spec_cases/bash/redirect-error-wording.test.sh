### redirect_onto_a_directory_fails
### exit_code: 1
mkdir d
echo x > d 2>/dev/null
### end

### trailing_slash_target_creates_nothing
echo y > nodir/ 2>/dev/null
echo "rc=$?"
ls
### expect
rc=1
### end

### append_to_a_trailing_slash_fails
echo z >> nodir/ 2>/dev/null
echo "rc=$?"
### expect
rc=1
### end
