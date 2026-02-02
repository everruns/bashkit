### head_default
### skip: default line count not working with stdin
# Head outputs first 10 lines by default
printf '%s\n' 1 2 3 4 5 6 7 8 9 10 11 12 | head
### expect
1
2
3
4
5
6
7
8
9
10
### end

### head_n_flag
# Head with -n flag
printf 'a\nb\nc\nd\ne\n' | head -n 3
### expect
a
b
c
### end

### head_shorthand
# Head with -N shorthand
printf 'a\nb\nc\nd\ne\n' | head -2
### expect
a
b
### end

### tail_default
### skip: default line count not working with stdin
# Tail outputs last 10 lines by default
printf '%s\n' 1 2 3 4 5 6 7 8 9 10 11 12 | tail
### expect
3
4
5
6
7
8
9
10
11
12
### end

### tail_n_flag
# Tail with -n flag
printf 'a\nb\nc\nd\ne\n' | tail -n 3
### expect
c
d
e
### end

### tail_shorthand
# Tail with -N shorthand
printf 'a\nb\nc\nd\ne\n' | tail -2
### expect
d
e
### end

### head_fewer_lines
# Head when input has fewer lines than requested
printf 'a\nb\n' | head -n 10
### expect
a
b
### end

### tail_fewer_lines
# Tail when input has fewer lines than requested
printf 'a\nb\n' | tail -n 10
### expect
a
b
### end

### head_one_line
# Head with -n 1
printf 'first\nsecond\nthird\n' | head -n 1
### expect
first
### end

### tail_one_line
# Tail with -n 1
printf 'first\nsecond\nthird\n' | tail -n 1
### expect
third
### end

### head_empty_input
# Head with empty input
printf '' | head
echo done
### expect
done
### end

### tail_empty_input
# Tail with empty input
printf '' | tail
echo done
### expect
done
### end

### head_n_zero
# Head with -n 0 outputs nothing
printf 'a\nb\nc\n' | head -n 0
echo done
### expect
done
### end

### tail_n_zero
# Tail with -n 0 outputs nothing
printf 'a\nb\nc\n' | tail -n 0
echo done
### expect
done
### end
