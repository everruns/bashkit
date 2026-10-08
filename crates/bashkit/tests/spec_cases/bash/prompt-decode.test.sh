# Prompt string decoding: ${x@P} (bash decode_prompt_string), then the
# promptvars expansion pass.

### prompt_literal_escapes
PS1='\a\e\r\n'
printf '%s' "${PS1@P}" | od -An -c | tr -s ' '
### expect
 \a 033 \r \n
### end

### prompt_octal_escapes
PS1='[\045]\1004$ \555 \0455'
echo "${PS1@P}"
### expect
[%]@4$ m %5
### end

### prompt_backslash_and_unknown
PS1='\\ \z'
echo "${PS1@P}"
### expect
\ \z
### end

### prompt_nonprinting_markers_vanish
PS1='\[x\]y'
echo "${PS1@P}"
### expect
xy
### end

### prompt_expansion_runs_after_decoding
x='\'
y='h'
PS1='$x$y'
echo "${PS1@P}"
### expect
\h
### end

### prompt_substituted_text_is_not_expanded
mkdir -p '/tmp/pd/$foo' && cd '/tmp/pd/$foo'
foo=foo_value
PS1='\W $foo'
echo "${PS1@P}"
### expect
$foo foo_value
### end

### prompt_working_dir
cd /tmp
PS1='\w|\W'
echo "${PS1@P}"
cd /
echo "${PS1@P}"
### expect
/tmp|tmp
/|/
### end

### prompt_home_is_tilde
HOME=/tmp
cd /tmp
PS1='\w \W'
echo "${PS1@P}"
### expect
~ ~
### end

### prompt_quotes_stay_literal
PS1='"q" a'"'"'b $((1+2)) $(echo cs)'
echo "${PS1@P}"
### expect
"q" a'b 3 cs
### end

### prompt_shell_and_version
PS1='\s \v \V'
echo "${PS1@P}" | grep -cE '^bash [0-9]+\.[0-9]+ [0-9]+\.[0-9]+\.[0-9]+$'
### expect
1
### end

### prompt_times
PS1='\t|\T|\@|\A|\d|\D{%H:%M}|\D{}'
echo "${PS1@P}" | grep -cE '^[0-2][0-9]:[0-5][0-9]:[0-5][0-9]\|[01][0-9]:[0-5][0-9]:[0-5][0-9]\|[01][0-9]:[0-5][0-9] (AM|PM)\|[0-2][0-9]:[0-5][0-9]\|[A-Z][a-z]+ [A-Z][a-z]+ [0-9]+\|[0-9]{2}:[0-9]{2}\|[0-9]{2}:[0-9]{2}:[0-9]{2}$'
### expect
1
### end

### prompt_user_matches_whoami
PS1='\u'
test "${PS1@P}" = "$(whoami)" && echo same
### expect
same
### end

### prompt_host_matches_hostname
PS1='\h|\H'
test "${PS1@P}" = "$(hostname -s)|$(hostname)" && echo same
### expect
same
### end

### prompt_jobs_and_tty
PS1='\j \l'
echo "${PS1@P}"
### expect
0 tty
### end

### prompt_promptvars_off
shopt -u promptvars
PS1='$HOME \\'
echo "${PS1@P}"
### expect
$HOME \
### end
