### cat_basic
# cat reads file content unchanged when no flags are passed
echo -e "alpha\nbeta\ngamma" > /tmp/cat1.txt
cat /tmp/cat1.txt
### expect
alpha
beta
gamma
### end

### cat_number_all
# -n numbers every output line; blank lines get the count + TAB then nothing
echo -e "x\n\ny" > /tmp/cat_n.txt
cat -n /tmp/cat_n.txt
### expect
     1	x
     2	
     3	y
### end

### cat_number_nonblank
# -b numbers only nonblank lines (overrides -n)
echo -e "x\n\ny" > /tmp/cat_b.txt
cat -b /tmp/cat_b.txt
### expect
     1	x

     2	y
### end

### cat_show_ends
# -E shows $ at end of each line
echo -e "a\nb" > /tmp/cat_E.txt
cat -E /tmp/cat_E.txt
### expect
a$
b$
### end

### cat_squeeze_blank
# -s collapses runs of blank lines into one
echo -e "a\n\n\n\nb" > /tmp/cat_s.txt
cat -s /tmp/cat_s.txt
### expect
a

b
### end

### cat_squeeze_with_number
# -ns: squeezed-out blanks must NOT be numbered; surviving blank gets a number
echo -e "a\n\n\n\nb" > /tmp/cat_ns.txt
cat -ns /tmp/cat_ns.txt
### expect
     1	a
     2	
     3	b
### end

### cat_show_all_composite
# -A is equivalent to -vET; shows tabs as ^I and ends as $
printf "a\tb\n" > /tmp/cat_A.txt
cat -A /tmp/cat_A.txt
### expect
a^Ib$
### end

### cat_unknown_flag_rejected
### bash_diff: clap-backed cat returns exit 2 for parse errors; GNU cat returns 1
# clap rejects unknown flags with a usage error and non-zero exit
cat --no-such-flag 2>/dev/null; echo "exit=$?"
### expect
exit=2
### end

### cat_stdin_dash
# `-` as file means stdin
echo "from-stdin" | cat -
### expect
from-stdin
### end
