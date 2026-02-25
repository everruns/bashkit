### extglob_at_basic
# @(a|b) matches exactly one alternative
shopt -s extglob
case "foo" in @(foo|bar)) echo "match";; *) echo "no";; esac
### expect
match
### end

### extglob_at_no_match
# @(a|b) doesn't match non-alternatives
shopt -s extglob
case "baz" in @(foo|bar)) echo "match";; *) echo "no";; esac
### expect
no
### end

### extglob_question_zero
# ?(a|b) matches zero occurrences
shopt -s extglob
case "" in ?(foo|bar)) echo "match";; *) echo "no";; esac
### expect
match
### end

### extglob_question_one
# ?(a|b) matches one occurrence
shopt -s extglob
case "foo" in ?(foo|bar)) echo "match";; *) echo "no";; esac
### expect
match
### end

### extglob_question_no_two
# ?(a|b) does NOT match two occurrences
shopt -s extglob
case "foobar" in ?(foo|bar)) echo "match";; *) echo "no";; esac
### expect
no
### end

### extglob_plus_one
# +(a|b) matches one occurrence
shopt -s extglob
case "foo" in +(foo|bar)) echo "match";; *) echo "no";; esac
### expect
match
### end

### extglob_plus_multiple
# +(a|b) matches multiple occurrences
shopt -s extglob
case "foobar" in +(foo|bar)) echo "match";; *) echo "no";; esac
### expect
match
### end

### extglob_plus_no_zero
# +(a|b) does NOT match zero
shopt -s extglob
case "" in +(foo|bar)) echo "match";; *) echo "no";; esac
### expect
no
### end

### extglob_star_zero
# *(a|b) matches zero occurrences
shopt -s extglob
case "" in *(foo|bar)) echo "match";; *) echo "no";; esac
### expect
match
### end

### extglob_star_multiple
# *(a|b) matches multiple occurrences
shopt -s extglob
case "foobarfoo" in *(foo|bar)) echo "match";; *) echo "no";; esac
### expect
match
### end

### extglob_not_basic
# !(a|b) matches anything except alternatives
shopt -s extglob
case "baz" in !(foo|bar)) echo "match";; *) echo "no";; esac
### expect
match
### end

### extglob_not_reject
# !(a|b) rejects exact matches
shopt -s extglob
case "foo" in !(foo|bar)) echo "match";; *) echo "no";; esac
### expect
no
### end

### extglob_conditional
# extglob in [[ == ]]
shopt -s extglob
[[ "hello" == @(hello|world) ]] && echo "yes" || echo "no"
### expect
yes
### end

### extglob_conditional_no
# extglob in [[ != ]]
shopt -s extglob
[[ "xyz" == @(hello|world) ]] && echo "yes" || echo "no"
### expect
no
### end

### extglob_off_literal
# Without extglob, @(...) is literal
case "@(foo)" in '@(foo)') echo "literal";; *) echo "no";; esac
### expect
literal
### end
