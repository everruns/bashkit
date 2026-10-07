### symlink_read_through_file_link
# cat, test and wc follow a link to a file
mkdir -p /tmp/sl1 && cd /tmp/sl1
echo hello > target.txt
ln -s target.txt rel
ln -s /tmp/sl1/target.txt abs
cat rel abs
[ -f rel ] && echo file
[ -L rel ] && echo link
[ -L target.txt ] || echo notlink
wc -c < abs
### expect
hello
hello
file
link
notlink
6
### end

### symlink_write_through_link
# redirects and appends land in the target
mkdir -p /tmp/sl2 && cd /tmp/sl2
echo one > t
ln -s t l
echo two >> l
cat t
readlink l
### expect
one
two
t
### end

### symlink_directory_link
# a link to a directory can be listed, entered and written into
mkdir -p /tmp/sl3/real/sub && cd /tmp/sl3
echo a > real/a.txt
ln -s real dl
ls dl
[ -d dl ] && echo dir
cat dl/a.txt
echo b > dl/b.txt
cat real/b.txt
mkdir -p dl/sub/deeper
[ -d real/sub/deeper ] && echo deep
cd dl && pwd && cat a.txt
### expect
a.txt
sub
dir
a
b
deep
/tmp/sl3/dl
a
### end

### symlink_dangling
# a dangling link exists as a link but not as a file; writing creates the target
mkdir -p /tmp/sl4 && cd /tmp/sl4
ln -s missing.txt dang
[ -e dang ] || echo noexist
[ -L dang ] && echo islink
cat dang 2>/dev/null || echo catfail
echo made > dang
cat missing.txt
### expect
noexist
islink
catfail
made
### end

### symlink_loop_is_reported
# loops fail with ELOOP instead of hanging
mkdir -p /tmp/sl5 && cd /tmp/sl5
ln -s b a
ln -s a b
cat a
echo "exit=$?"
[ -e a ] || echo noexist
### expect
exit=1
noexist
### end

### symlink_loop_message
mkdir -p /tmp/sl6 && cd /tmp/sl6
ln -s b a
ln -s a b
cat a 2>&1
### expect
cat: a: Too many levels of symbolic links
### end

### symlink_rm_and_mv_act_on_link
# rm and mv never touch the target
mkdir -p /tmp/sl7 && cd /tmp/sl7
echo keep > t
ln -s t l
mv l l2
readlink l2
rm l2
cat t
[ -e l2 ] || echo gone
### expect
t
keep
gone
### end

### symlink_ls_long_shows_target
mkdir -p /tmp/sl8 && cd /tmp/sl8
touch t
ln -s t l
ls -l l | sed 's/.* l -> /l -> /'
### expect
l -> t
### end

### symlink_relative_chain
# relative targets resolve against the link's own directory, through chains
mkdir -p /tmp/sl9/a/b && cd /tmp/sl9
echo deep > a/b/f
ln -s b/f a/l1
ln -s a/l1 l2
cat l2
readlink -f l2
### expect
deep
/tmp/sl9/a/b/f
### end
