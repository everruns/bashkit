# pr: paginate and columnate (behavior matches GNU pr; header dates use -D)

### pr_plain_omit_header
seq 3 | pr -t
### expect
1
2
3
### end

### pr_page_layout
seq 3 | pr -l 13 -D DATE -h TITLE
### expect


DATE                            TITLE                             Page 1


1
2
3





### end

### pr_two_columns
seq 10 | pr -t -2
### expect
1				    6
2				    7
3				    8
4				    9
5				    10
### end

### pr_three_columns_balanced
seq 7 | pr -t -3
### expect
1			4			6
2			5			7
3
### end

### pr_across
seq 7 | pr -t -3 -a
### expect
1			2			3
4			5			6
7
### end

### pr_narrow_columns
seq 7 | pr -t -4 -w 30
### expect
1      3      5	     7
2      4      6
### end

### pr_truncate_columns
printf 'a       b\nlong line here that is quite long indeed more than thirty six chars\n4\n' | pr -t -2
### expect
a	b			    4
long line here that is quite long i
### end

### pr_join_lines
printf 'a       b\nlong line here that is quite long indeed more than thirty six chars\n4\n' | pr -t -2 -J
### expect
a	b	4
long line here that is quite long indeed more than thirty six chars
### end

### pr_number_lines
printf 'a\nb\n' | pr -t -n
### expect
    1	a
    2	b
### end

### pr_number_custom
seq 3 | pr -t -n:3 -N 9
### expect
  9:1
 10:2
 11:3
### end

### pr_number_columns
seq 6 | pr -t -2 -n
### expect
    1	1			    	4   4
    2	2			    	5   5
    3	3			    	6   6
### end

### pr_double_space
seq 3 | pr -t -d
### expect
1

2

3

### end

### pr_offset
seq 2 | pr -t -o 3
### expect
   1
   2
### end

### pr_separator_char
seq 4 | pr -t -2 -s,
### expect
1,3
2,4
### end

### pr_separator_string
seq 4 | pr -t -2 -S' | '
### expect
1				   | 3
2				   | 4
### end

### pr_merge
printf '1\n2\n' > /tmp/pr_a; seq 4 > /tmp/pr_b; pr -m -t /tmp/pr_a /tmp/pr_b
### expect
1				    1
2				    2
				    3
				    4
### end

### pr_merge_numbered
printf '1\n2\n' > /tmp/pr_a; seq 3 > /tmp/pr_b; pr -m -n -t /tmp/pr_a /tmp/pr_b
### expect
    1	1				1
    2	2				2
    3					3
### end

### pr_pages
seq 5 | pr -l 12 -D d
### expect


d                                                                 Page 1


1
2







d                                                                 Page 2


3
4







d                                                                 Page 3


5






### end

### pr_page_range
seq 5 | pr -l 12 -D d +2:2
### expect


d                                                                 Page 2


3
4





### end

### pr_form_feed
seq 2 | pr -F -D d | od -c | tail -2 | head -1
### expect
0000120  \n  \f
### end

### pr_short_page_omits_header
seq 3 | pr -l 5
### expect
1
2
3
### end

### pr_bad_length
LC_ALL=C pr -l abc /dev/null 2>&1; echo "rc=$?"
### expect
pr: '-l PAGE_LENGTH' invalid number of lines: 'abc'
rc=1
### end

### pr_bad_columns
LC_ALL=C pr -0 /dev/null 2>&1; echo "rc=$?"
### expect
pr: invalid number of columns: '0': Numerical result out of range
rc=1
### end

### pr_bad_option
LC_ALL=C pr -Z /dev/null 2>&1; echo "rc=$?"
### expect
pr: invalid option -- 'Z'
Try 'pr --help' for more information.
rc=1
### end

### pr_missing_file
LC_ALL=C pr -t /tmp/pr_nosuch 2>&1; echo "rc=$?"
### expect
pr: /tmp/pr_nosuch: No such file or directory
rc=1
### end
