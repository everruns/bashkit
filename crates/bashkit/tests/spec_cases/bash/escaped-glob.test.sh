# Quoted or backslash-escaped glob characters are literal: in words
# (`a\*`, `a"*"`), case patterns and [[ == ]] patterns. Unquoted glob
# characters in the same word still glob.

### escaped_star_not_globbed
mkdir -p /tmp/el && cd /tmp/el && touch ab ac
echo a\*
echo a"*"
echo a'?'
echo a*
### expect
a*
a*
a?
ab ac
### end

### escaped_glob_in_case_pattern
case ab in a\*) echo glob ;; *) echo literal ;; esac
case 'a*' in a\*) echo star ;; *) echo no ;; esac
case '[s]' in \[s\]) echo bracket ;; *) echo no ;; esac
case ab in a"*") echo glob ;; *) echo literal ;; esac
### expect
literal
star
bracket
literal
### end

### quoted_var_in_case_pattern_is_literal
x='a*'
case ab in "$x") echo glob ;; *) echo literal ;; esac
case ab in $x) echo glob ;; *) echo literal ;; esac
case 'a*' in "$x") echo exact ;; *) echo no ;; esac
### expect
literal
glob
exact
### end

### escaped_glob_in_double_bracket
[[ ab == a\* ]] && echo glob || echo literal
[[ '[s]' == \[s\] ]] && echo bracket || echo no
x='a*'
[[ ab == "$x" ]] && echo glob || echo literal
[[ ab == $x ]] && echo glob || echo literal
[[ ab == a"*" ]] && echo glob || echo literal
### expect
literal
bracket
literal
glob
literal
### end

### quoted_glob_prefix_then_unquoted_glob
mkdir -p /tmp/qg && cd /tmp/qg && touch 'a*b' axb
echo "*"zz*
echo "a*"*
x='a*'; echo "$x"zz*
echo a"*"*
echo a\**
### expect
*zz*
a*b
a*zz*
a*b
a*b
### end

### quoted_glob_assignment_keeps_text
x="a*"b*; echo "$x"
y=a\*b?; echo "$y"
### expect
a*b*
a*b?
### end
