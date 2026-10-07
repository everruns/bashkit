### srandom_is_32bit_number
# $SRANDOM (bash 5.1+) is a fresh 32-bit value from the system CSPRNG.
[[ $SRANDOM =~ ^[0-9]+$ ]] && echo num
(( SRANDOM <= 4294967295 )) && echo fits
[ "$SRANDOM" != "$SRANDOM" ] && echo differs
### expect
num
fits
differs
### end

### srandom_assignment_ignored
SRANDOM=5
[ "$SRANDOM" != 5 ] && echo ignored
### expect
ignored
### end

### openssl_rand_hex
openssl rand -hex 16 | grep -cE '^[0-9a-f]{32}$'
### expect
1
### end

### openssl_rand_base64_and_raw
openssl rand -base64 12 | grep -cE '^[A-Za-z0-9+/]{16}$'
openssl rand 7 | wc -c
openssl rand -hex 0 | wc -c
### expect
1
7
0
### end

### openssl_rand_out_file
openssl rand -out /tmp/key.hex -hex 3
grep -cE '^[0-9a-f]{6}$' /tmp/key.hex
### expect
1
### end

### openssl_rand_bad_count
openssl rand -hex abc
echo rc=$?
### expect
rc=1
### end

### openssl_unknown_command
openssl nope 2>/dev/null
echo rc=$?
### expect
rc=1
### end

### uuidgen_random_v4
### bash_diff: uuidgen is not installed on every host
uuidgen | grep -cE '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
uuidgen -r | wc -c
[ "$(uuidgen)" != "$(uuidgen)" ] && echo unique
### expect
1
37
unique
### end

### password_idioms
# Common secret-generation one-liners.
tr -dc 'A-Za-z0-9' </dev/urandom | head -c 20 | wc -c
head -c 32 /dev/urandom | base64 | tr -d '\n' | wc -c
### expect
20
44
### end

### uuidgen_time_based_unsupported
### bash_diff: L-RAND-001, only random (v4) UUIDs
uuidgen -t 2>/dev/null
echo rc=$?
### expect
rc=1
### end
