### truncate_extends_short_file
echo -n "abc" > /tmp/tr1.txt
truncate -s 8 /tmp/tr1.txt
wc -c < /tmp/tr1.txt
### expect
8
### end

### truncate_shrinks_long_file
echo -n "0123456789" > /tmp/tr2.txt
truncate -s 4 /tmp/tr2.txt
cat /tmp/tr2.txt
echo
### expect
0123
### end

### truncate_creates_missing_file
truncate -s 5 /tmp/tr3.txt
wc -c < /tmp/tr3.txt
### expect
5
### end

### truncate_no_create_skips_missing
truncate --no-create -s 5 /tmp/tr4_missing.txt
test ! -e /tmp/tr4_missing.txt && echo "ok"
### expect
ok
### end

### truncate_extend_relative
echo -n "abc" > /tmp/tr5.txt
truncate -s +2 /tmp/tr5.txt
wc -c < /tmp/tr5.txt
### expect
5
### end

### truncate_reduce_relative
echo -n "abcdef" > /tmp/tr6.txt
truncate -s -2 /tmp/tr6.txt
cat /tmp/tr6.txt
echo
### expect
abcd
### end

### truncate_reduce_below_zero_clamps
echo -n "abc" > /tmp/tr7.txt
truncate -s -100 /tmp/tr7.txt
wc -c < /tmp/tr7.txt
### expect
0
### end

### truncate_at_most_clamps_larger
echo -n "abcdef" > /tmp/tr8.txt
truncate -s '<4' /tmp/tr8.txt
cat /tmp/tr8.txt
echo
### expect
abcd
### end

### truncate_at_least_extends_smaller
echo -n "abc" > /tmp/tr9.txt
truncate -s '>5' /tmp/tr9.txt
wc -c < /tmp/tr9.txt
### expect
5
### end

### truncate_kib_unit
truncate -s 1K /tmp/tr10.txt
wc -c < /tmp/tr10.txt
### expect
1024
### end

### truncate_kb_decimal_unit
truncate -s 1KB /tmp/tr11.txt
wc -c < /tmp/tr11.txt
### expect
1000
### end

### truncate_reference_size
echo -n "abcde" > /tmp/tr12_ref.txt
truncate -r /tmp/tr12_ref.txt /tmp/tr12_dest.txt
wc -c < /tmp/tr12_dest.txt
### expect
5
### end

### truncate_unknown_flag_rejected
# Clap exits 2 on unknown flag; GNU truncate exits 1. The PR's #1532
# scope notes the clap-vs-GNU exit-code divergence; we assert
# non-zero here to keep the row stable.
### bash_diff
truncate --bogus /tmp/tr_bogus.txt
echo "exit=$?"
### expect
exit=2
### end
