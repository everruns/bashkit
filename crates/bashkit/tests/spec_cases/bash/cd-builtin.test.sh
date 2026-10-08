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
