# Tilde expansion: a leading unquoted `~`, the `~` after `=` and `:` in
# assignments (and in assignment-looking arguments), `${x:-~}` operands and
# pattern operators. Quoted or escaped tildes, and prefixes that run into
# quoted text or an expansion, stay literal.

### tilde_quoted_forms_stay_literal
HOME=/home/tt
x=y
echo ~/"b" ~"/b" ~/$x ~$x "~" \~ ~:x
### expect
/home/tt/b ~/b /home/tt/y ~y ~ ~ /home/tt:x
### end

### tilde_in_assignment_values
HOME=/home/tt
v=a:~:~root/x:"~":\~
echo $v
v='~'
echo $v
v=\~
echo $v
v=~/"b"
echo $v
PATHLIKE=~/bin:~/sbin
echo $PATHLIKE
### expect
a:/home/tt:/root/x:~:~
~
~
/home/tt/b
/home/tt/bin:/home/tt/sbin
### end

### tilde_in_assignment_like_arguments
HOME=/home/tt
echo x=~ x=~:~ a=b:~ 1x=~ foo:~
readonly r=~:~
echo $r
f() { local l=x:~; echo $l; }
f
### expect
x=/home/tt x=/home/tt:/home/tt a=b:/home/tt 1x=~ foo:~
/home/tt:/home/tt
x:/home/tt
### end

### tilde_in_prefix_and_indexed_assignment
HOME=/home/tt
xx=~root:~ env | grep '^xx='
a[0]=foo:~
echo ${a[0]}
### expect
xx=/root:/home/tt
foo:/home/tt
### end

### tilde_in_parameter_operands
HOME=/home/tt
echo ${undef:-~} "${undef:-~}" ${HOME:+~/z} ${undef:-"~"}
x=/home/tt/a
echo ${x//~/z} "${x#~}" "${x/#~/Z}"
### expect
/home/tt ~ /home/tt/z ~
z/a /a /home/tt/a
### end

### tilde_brace_expansion_then_tilde
HOME=/home/tt
echo ~{/src,root}
### expect
/home/tt/src /root
### end
