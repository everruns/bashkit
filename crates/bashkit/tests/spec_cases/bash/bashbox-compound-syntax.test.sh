# BashBox compound-syntax cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_compound_syntax_until_loop
# until loop
i=0; until (( i >= 3 )); do echo $i; i=$((i+1)); done
### expect
0
1
2
### end

### bashbox_compound_syntax_c_style_for
# c-style for
for ((i=0; i<6; i+=2)); do echo $i; done
### expect
0
2
4
### end

### bashbox_compound_syntax_case
# [[ && ( || ) ]]
[[ -n a && ( -z "" || x == y ) ]] && echo yes
### expect
yes
### end

### bashbox_compound_syntax_case_2
# [[ || ! ]]
[[ a == b || ! -n "" ]]; echo $?; [[ a == b || -z x ]]; echo $?
### expect
0
1
### end

### bashbox_compound_syntax_bare_word
# [[ bare word ]]
[[ word ]] && echo set; [[ "" ]] || echo empty
### expect
set
empty
### end

### bashbox_compound_syntax_quoted_and_escaped_parens_inside
# quoted and escaped parens inside $(...)
echo $(echo 'a)' "b)" \) )
### expect
a) b) )
### end

### bashbox_compound_syntax_a_brace_inside_inside
# a brace inside $(...) inside ${...}
echo ${x:-$(echo })}
### expect
}
### end

### bashbox_compound_syntax_quoted_and_escaped_braces_inside
# quoted and escaped braces inside ${...}
y=ab; echo ${x:-"}"} ${x:-\}} ${x:-'a}'} ${y/a/\}} "${y/b/'}'}"
### expect
} } a} }b a}
### end

### bashbox_compound_syntax_single_quotes_stay_literal_in_a_double_quoted_default_word
# single quotes stay literal in a double-quoted default word
y=1; echo "${x:-'}'}" "${x:-"a b"}" "${x:-'a'"b"}" "${y:+'a'}"
### expect
'}' a b 'a'b 'a'
### end
