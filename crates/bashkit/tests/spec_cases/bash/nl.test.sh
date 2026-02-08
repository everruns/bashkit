### nl_basic
# Number lines with default settings
printf 'hello\nworld\n' | nl
### expect
     1	hello
     2	world
### end

### nl_skip_empty
# Default: skip empty lines
printf 'hello\n\nworld\n' | nl
### expect
     1	hello
       
     2	world
### end

### nl_all_lines
# Number all lines including empty
printf 'hello\n\nworld\n' | nl -b a
### expect
     1	hello
     2	
     3	world
### end

### nl_no_numbering
# No numbering
printf 'hello\nworld\n' | nl -b n
### expect
       hello
       world
### end

### nl_left_justified
# Left-justified numbers
printf 'hello\nworld\n' | nl -n ln
### expect
1     	hello
2     	world
### end

### nl_right_zero
# Zero-padded numbers
printf 'hello\nworld\n' | nl -n rz
### expect
000001	hello
000002	world
### end

### nl_custom_separator
# Custom separator
printf 'hello\nworld\n' | nl -s ': '
### expect
     1: hello
     2: world
### end

### nl_custom_increment
# Custom increment
printf 'a\nb\nc\n' | nl -i 2
### expect
     1	a
     3	b
     5	c
### end

### nl_custom_start
# Custom starting number
printf 'a\nb\nc\n' | nl -v 10
### expect
    10	a
    11	b
    12	c
### end

### nl_custom_width
# Custom width
printf 'a\nb\n' | nl -w 3
### expect
  1	a
  2	b
### end

### nl_empty_input
# Empty input produces no output
printf '' | nl
echo done
### expect
done
### end

### nl_single_line
# Single line
printf 'hello\n' | nl
### expect
     1	hello
### end

### nl_combined_options
# Combined options
printf 'x\ny\nz\n' | nl -b a -n rz -w 4 -v 5 -i 3
### expect
0005	x
0008	y
0011	z
### end

### nl_pipeline
# nl in pipeline
printf 'c\na\nb\n' | sort | nl
### expect
     1	a
     2	b
     3	c
### end
