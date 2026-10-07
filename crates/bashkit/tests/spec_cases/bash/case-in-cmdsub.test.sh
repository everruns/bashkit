### case_in_cmdsub_one_line
x=b
echo "$(case $x in a) echo A;; b) echo B;; esac)"
### expect
B
### end

### case_in_cmdsub_multiline_nested
r=$(case 1 in
  1) case 2 in
       2) echo inner;;
     esac;;
  *) echo other;;
esac)
echo "$r"
### expect
inner
### end

### case_in_cmdsub_paren_pattern_and_comment
v=$(case z in (y|z) echo yz ;; # a) comment
esac)
echo "$v"
### expect
yz
### end

### case_in_backticks
echo `case q in q) echo Q;; esac`
### expect
Q
### end

### case_word_as_argument
echo $(echo case esac in)
### expect
case esac in
### end

### dbracket_posix_class_pattern
z=5abc
[[ $z == [[:digit:]]* ]] && echo digit
[[ a1 == [[:alpha:]][[:digit:]] ]] && echo alnum
[[ x == [[:digit:]] ]] || echo nodigit
### expect
digit
alnum
nodigit
### end

### case_posix_class_pattern
case 5 in [[:digit:]]) echo d;; *) echo n;; esac
case k in [[:digit:]]) echo d;; *) echo n;; esac
### expect
d
n
### end
