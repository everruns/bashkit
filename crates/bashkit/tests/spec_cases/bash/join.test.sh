### join_output_format_and_empty
# -o picks fields, -e fills the ones a side lacks, -a keeps unpaired lines
printf '1\ta\n2\tb\n4\td\n' > /tmp/j1; printf '1\tx\n3\ty\n4\tz\n' > /tmp/j2
join -t "$(printf '\t')" -a1 -a2 -e NA -o 0,1.2,2.2 /tmp/j1 /tmp/j2
### expect
1	a	x
2	b	NA
3	NA	y
4	d	z
### end

### join_many_to_many
printf 'k 1\nk 2\nm 3\n' > /tmp/m1; printf 'k a\nk b\n' > /tmp/m2
join /tmp/m1 /tmp/m2
### expect
k 1 a
k 1 b
k 2 a
k 2 b
### end

### join_blank_runs_and_unpaired_only
printf '  a   1\nb 2\n' > /tmp/b1; printf 'a  x\nc y\n' > /tmp/b2
join /tmp/b1 /tmp/b2
join -v 1 /tmp/b1 /tmp/b2
join -v 2 /tmp/b1 /tmp/b2
### expect
a 1 x
b 2
c y
### end

### join_field_selection_and_ignore_case
printf '1 A\n2 B\n' > /tmp/f1; printf 'a red\nb blue\n' > /tmp/f2
join -i -1 2 -2 1 -o 1.1,2.2 /tmp/f1 /tmp/f2
join -j 1 -o auto /tmp/f2 /tmp/f2
### expect
1 red
2 blue
a red red
b blue blue
### end

### join_header
printf 'id name\n1 a\n' > /tmp/h1; printf 'id score\n1 9\n' > /tmp/h2
join --header /tmp/h1 /tmp/h2
### expect
id name score
1 a 9
### end
