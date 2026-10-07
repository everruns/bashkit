# BashBox shopt-pattern cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_shopt_pattern_always_understands_extglob
# [[ ]] always understands extglob
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
x=abc; [[ $x == @(a|b)* ]] && echo y1; [[ $x == +([a-c]) ]] && echo y2; [[ $x == !(a*) ]] || echo y3; [[ "" == !(x) ]] && echo y4; [[ "a|b" == @(a\|b) ]] && echo y5; [[ abab == +(ab|a) ]] && echo y6; [[ b == *(a)b ]] && echo y7; [[ ab == ?(a)b ]] && echo y8
### expect
y1
y2
y3
y4
y5
y6
y7
y8
### end

### bashbox_shopt_pattern_case_needs_shopt_s_extglob
# case needs shopt -s extglob
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
p='@(abc)'; case abc in $p) echo no;; *) echo plain;; esac
shopt -s extglob
case abc in $p) echo yes;; esac; case abc in @(abc)) echo yes;; esac; case a in !(a)*) echo m1;; esac
### expect
plain
yes
yes
m1
### end

### bashbox_shopt_pattern_pattern_removal_and_substitution_with_extglob
# pattern removal and substitution with extglob
shopt -s extglob
x=abc; echo "${x//*(z)/-}|${x/*(z)/-}|${x//!(b)/-}|${x/!(b)/-}|${x#!(b)}|${x##!(b)}|${x%!(b)}|${x%%!(b)}|${x/#!(a)/-}|${x/%!(c)/-}"
echo ${x/@(b|c)/X} ${x//[ac]/Y}; y=aXbXc; echo ${y//!(X)/-} ${x//?(b)/-}
x=aab; echo ${x##+(a)} ${x#+(a)} ${x%%+(b)} ${x/+(a)/-}; z=a; echo "${z//!(a)/-}" "${z/#!(b)/-}" "${x/%!(z)/-}" "${x/!(a*)/-}" "${x/!(*)/-}"
### expect
-a-b-c|-abc|-|-|abc||abc||-|-
aXc YbY
- -a--c
b ab aa -b
-a - - -aab aab
### end

### bashbox_shopt_pattern_quoted_parts_of_a_pattern_match_literally
# quoted parts of a pattern match literally
[[ abc == "a"* ]] && echo q1; [[ abc == "a*" ]] || echo q2; case "a*" in "a*") echo q3;; esac; case ab in "a"?) echo q4;; esac
p='a*'; [[ abc == $p ]] && echo q5; [[ abc == "$p" ]] || echo q6; [[ x != "x" ]] || echo q7
### expect
q1
q2
q3
q4
q5
q6
q7
### end

### bashbox_shopt_pattern_nocasematch_covers_case_and_substitution_not_removal
# nocasematch covers case, [[ ]] and substitution, not removal
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
x=ABC; shopt -s nocasematch
case ABC in a*) echo m1;; esac; [[ ABC == ab? ]] && echo m2; [[ ABC =~ ^ab ]] && echo m3; [[ ABC = abc ]] && echo m4; echo ${x#a} ${x/b/z} ${x%c} ${x//b/z} ${x/#a/q}; [[ $x != abc ]]; echo $?
shopt -u nocasematch; [[ ABC == abc ]] || echo m5
### expect
m1
m2
m3
m4
ABC AzC ABC AzC qBC
1
m5
### end

### bashbox_shopt_pattern_bracket_expressions_in_removal_patterns
# bracket expressions in removal patterns
x=ABC; echo ${x#[AB]} ${x##[[:upper:]]*} ${x%[!C]C} ${x/[z-a]/q} ${x#[z-a]}
### expect
BC A ABC ABC
### end

### bashbox_shopt_pattern_array_and_positional_operations_apply_per_element
# array and positional operations apply per element
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
set -- ab ac; echo "${@#a}" "${@/c/x}"; a=(xa ya); echo "${a[@]%a}" "${a[*]^^}"; x=aab; echo ${x/q/z} ${x/#/-} ${x/%/-} "${x//b/\$}"; declare -A m=([k1]=a [k2]=b); for k in "${!m[@]}"; do echo "<$k>"; done
### expect
b c ab ax
x y XA YA
aab -aab aab- aa$
<k1>
<k2>
### end

### bashbox_shopt_pattern_quotes_inside_a_group
# quotes inside a group
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
[[ "b)" == @(a|'b)') ]] && echo q1; [[ 'a"' == @("a\"") ]] && echo q2; [[ 'a|' == @(a\|) ]] && echo q3
### expect
q1
q2
q3
### end

### bashbox_shopt_pattern_an_unbalanced_group_from_a_variable_is_literal
# an unbalanced group from a variable is literal
p='@(a'; [[ '@(a' == $p ]] && echo lit
### expect
lit
### end

### bashbox_shopt_pattern_a_negation_nested_in_another_group
# a negation nested in another group
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
[[ c == @(!(a)|b) ]] && echo y1; [[ a == @(!(a)) ]] || echo y2; [[ ab == !(x)? ]] && echo y3
### expect
y1
y2
y3
### end

### bashbox_shopt_pattern_groups_next_to_a_negation
# groups next to a negation
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
[[ abab == !(x)+(ab) ]] && echo y1; [[ ab == @(a)!(x) ]] && echo y2; [[ b == ?(a)!(z) ]] && echo y3; [[ aab == *(a)!(z) ]] && echo y4; [[ ab == +(a|ab)!(z) ]] && echo y5; [[ aaa == !(z)*(a|aa) ]] && echo y6; [[ x == !(z)+(a) ]] || echo y7; [[ ab == !(x)+(a|b) ]] && echo y8
### expect
y1
y2
y3
y4
y5
y6
y7
y8
### end

### bashbox_shopt_pattern_no_match_leaves_the_value_alone
# no match leaves the value alone
shopt -s extglob
x=abc; echo ${x#!(*)} ${x/#!(*)/-} ${x//!(*)/-} ${x%%!(*)}
### expect
abc abc abc abc
### end
