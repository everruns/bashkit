# fmt: optimal paragraph filling (line breaker vendored from uutils fmt).
# Cases marked bash_diff break lines differently from GNU fmt.

### fmt_joins_lines
printf 'one two\nthree four\nfive\n' | fmt
### expect
one two three four five
### end

### fmt_width
### bash_diff: uutils line breaker picks different breaks than GNU fmt
printf 'The quick brown fox jumps over the lazy dog and keeps running far away.\n' | fmt -w 30
### expect
The quick brown fox jumps over
the lazy dog and keeps running
far away.
### end

### fmt_obsolete_width
### bash_diff: uutils line breaker picks different breaks than GNU fmt
printf 'alpha beta gamma delta epsilon zeta eta theta iota kappa\n' | fmt -20
### expect
alpha beta gamma
delta epsilon zeta
eta theta iota kappa
### end

### fmt_sentences
printf 'One.  Two words here.\nThree.\n' | fmt
### expect
One.  Two words here.  Three.
### end

### fmt_uniform
printf 'a   b    c.  d\n' | fmt -u
### expect
a b c.  d
### end

### fmt_split_only
printf 'short\nalpha beta gamma delta epsilon zeta eta theta\n' | fmt -s -w 20
### expect
short
alpha beta gamma
delta epsilon zeta
eta theta
### end

### fmt_paragraphs_and_indent
printf 'a b\nc d\n\n   indented x\n   y z\n' | fmt
### expect
a b c d

   indented x y z
### end

### fmt_crown
### bash_diff: uutils line breaker picks different breaks than GNU fmt
printf '  first line\n    second line words here and more words to wrap\n' | fmt -c -w 30
### expect
  first line second line words
    here and more words to
    wrap
### end

### fmt_tagged
### bash_diff: uutils line breaker picks different breaks than GNU fmt
printf 'tag: alpha beta gamma delta epsilon zeta eta theta\n' | fmt -t -w 25
### expect
tag: alpha beta gamma
    delta epsilon zeta
    eta theta
### end

### fmt_prefix
printf '> alpha beta\n> gamma delta\ncode line\n> epsilon\n' | fmt -p '> '
### expect
> alpha beta gamma delta
code line
> epsilon
### end

### fmt_goal
printf 'one two three four five six seven eight nine ten eleven twelve\n' | fmt -g 20
### expect
one two three four
five six seven eight
nine ten eleven twelve
### end

### fmt_file
printf 'x y\nz\n' > /tmp/fmt_in.txt
fmt -w 10 /tmp/fmt_in.txt
### expect
x y z
### end

### fmt_bad_width
LC_ALL=C fmt -w abc </dev/null 2>&1; echo "rc=$?"
### expect
fmt: invalid width: 'abc'
rc=1
### end

### fmt_width_too_large
LC_ALL=C fmt -w 3000 </dev/null 2>&1; echo "rc=$?"
### expect
fmt: invalid width: '3000': Numerical result out of range
rc=1
### end

### fmt_goal_over_width
LC_ALL=C fmt -w 20 -g 30 </dev/null 2>&1; echo "rc=$?"
### expect
fmt: invalid width: '30': Numerical result out of range
rc=1
### end

### fmt_missing_file
LC_ALL=C fmt /tmp/fmt_nosuch_file 2>&1; echo "rc=$?"
### expect
fmt: cannot open '/tmp/fmt_nosuch_file' for reading: No such file or directory
rc=1
### end
