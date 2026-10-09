# cd and pwd builtin tests: options, OLDPWD, CDPATH, logical vs physical paths

### cd_option_and_operand_errors
cd -x 2>/dev/null; echo "invalid=$?"
cd /tmp /tmp 2>/dev/null; echo "two=$?"
unset OLDPWD; cd - 2>/dev/null; echo "oldpwd=$?"
cd /nonexistent_bk/.. 2>/dev/null; echo "missing_dotdot=$?"
cd -- /; echo "dashdash=$? $PWD"
### expect
invalid=2
two=1
oldpwd=1
missing_dotdot=1
dashdash=0 /
### end

### cd_dash_prints_directory
cd /tmp; cd /; cd -; pwd
### expect
/tmp
/tmp
### end

### cd_cdpath_lookup
d=/tmp/bk_cdpath_$$; mkdir -p $d/spam/foo $d/eggs/foo
CDPATH="$d/spam:$d/eggs"; cd foo >/dev/null; echo "$? ${PWD#$d}"
cd $d; CDPATH=$d/eggs; cd ./foo 2>/dev/null; echo "dot=$?"
cd /; rm -rf $d
### expect
0 /spam/foo
dot=1
### end

### cd_physical_and_pwd_p
d=/tmp/bk_cdp_$$; mkdir -p $d/target/sub; ln -s $d/target $d/link
cd $d/link/sub; echo "${PWD#$d}"
cd -L ..; echo "${PWD#$d}"
cd -P $d/link; echo "${PWD#$d}"
cd $d/link; echo "$(basename "$(pwd)") $(basename "$(pwd -P)")"
cd /; rm -rf $d
### expect
/link/sub
/link
/target
link target
### end

### cd_cdpath_empty_order_and_fallback
d=/tmp/bk_cdpath_order_$$; mkdir -p $d/here/leaf $d/first/leaf $d/second/leaf
cd $d/here; CDPATH=":$d/first:$d/second"; out=$(cd leaf); echo "empty=<$out>"
CDPATH="$d/missing:$d/first:$d/second"; out=$(cd leaf); echo "hit=${out#$d}"
CDPATH=$d/missing; cd leaf; echo "fallback=${PWD#$d}"
cd $d/here; CDPATH=$d/first; cd ./leaf; echo "dot=${PWD#$d}"
cd $d; echo "absolute=${PWD#$d}"
cd /; rm -rf $d
### expect
empty=<>
hit=/first/leaf
fallback=/here/leaf
dot=/here/leaf
absolute=
### end
