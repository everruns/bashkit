### array_slice_basic
arr=(a b c d e)
echo "${arr[@]:1:3}"
### expect
b c d
### end

### array_slice_from_start
arr=(a b c d e)
echo "${arr[@]:0:2}"
### expect
a b
### end

### array_slice_to_end
arr=(a b c d e)
echo "${arr[@]:2}"
### expect
c d e
### end

### array_slice_negative_offset
arr=(a b c d e)
echo "${arr[@]: -2}"
### expect
d e
### end

### array_slice_single
arr=(a b c d e)
echo "${arr[@]:3:1}"
### expect
d
### end

### array_slice_zero_length
arr=(a b c d e)
echo ">${arr[@]:1:0}<"
### expect
><
### end

### array_slice_beyond_bounds
arr=(a b c)
echo "${arr[@]:1:10}"
### expect
b c
### end

### array_slice_at_length
arr=(a b c)
echo ">${arr[@]:3}<"
### expect
><
### end
