### comm_basic
# comm shows three-column output for sorted files
echo -e "a\nb\nc" > /tmp/comm1.txt
echo -e "b\nc\nd" > /tmp/comm2.txt
comm /tmp/comm1.txt /tmp/comm2.txt
### expect
a
		b
		c
	d
### end

### comm_suppress_col1
# comm -1 suppresses lines unique to file1
echo -e "a\nb\nc" > /tmp/c1s1.txt
echo -e "b\nc\nd" > /tmp/c1s2.txt
comm -1 /tmp/c1s1.txt /tmp/c1s2.txt
### expect
	b
	c
d
### end

### comm_suppress_col2
# comm -2 suppresses lines unique to file2
echo -e "a\nb\nc" > /tmp/c2s1.txt
echo -e "b\nc\nd" > /tmp/c2s2.txt
comm -2 /tmp/c2s1.txt /tmp/c2s2.txt
### expect
a
	b
	c
### end

### comm_suppress_col3
# comm -3 suppresses common lines
echo -e "a\nb\nc" > /tmp/c3s1.txt
echo -e "b\nc\nd" > /tmp/c3s2.txt
comm -3 /tmp/c3s1.txt /tmp/c3s2.txt
### expect
a
	d
### end

### comm_only_common
# comm -12 shows only common lines
echo -e "a\nb\nc" > /tmp/c12a.txt
echo -e "b\nc\nd" > /tmp/c12b.txt
comm -12 /tmp/c12a.txt /tmp/c12b.txt
### expect
b
c
### end

### comm_identical_files
# comm with identical files shows all in column 3
echo -e "x\ny" > /tmp/ci1.txt
echo -e "x\ny" > /tmp/ci2.txt
comm /tmp/ci1.txt /tmp/ci2.txt
### expect
		x
		y
### end
