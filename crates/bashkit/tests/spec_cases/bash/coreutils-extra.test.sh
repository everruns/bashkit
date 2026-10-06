# Checksums, encodings, cmp, factor, tsort, nproc: GNU-compatible output.

### sha2_family_and_b2sum
printf 'hello\n' | sha224sum
printf 'hello\n' | sha384sum
printf 'hello\n' | sha512sum
printf 'hello\n' | b2sum
### expect
2d6d67d91d0badcdd06cbbba1fe11538a68a37ec9c2e26457ceff12b  -
1d0f284efe3edea4b9ca3bd514fa134b17eae361ccc7a1eefeff801b9bd6604e01f21f6bf249ef030599f0c218f2ba8c  -
e7c22b994c59d9cf2b48e549b1e24666636045930d3da7c1acb299d1c3b7f931f94aae41edda2c2b207a36e10f8bcb8d45223e54878f5b316e7ce3b6bc019629  -
f60ce482e5cc1229f39d71313171a8d9f4ca3a87d066bf4b205effb528192a75f14f3271e2c1a90e1de53f275b4d4793eef2f5e31ea90d2ce29d2e481c36435f  -
### end

### sha256sum_check
d=$(mktemp -d); cd "$d"
printf 'hello\n' > a; printf 'x\n' > b
sha256sum a b > SUMS
sha256sum -c SUMS; echo rc=$?
printf 'y\n' > b
sha256sum -c SUMS 2>&1; echo rc=$?
sha256sum --status -c SUMS; echo rc=$?
### expect
a: OK
b: OK
rc=0
a: OK
b: FAILED
sha256sum: WARNING: 1 computed checksum did NOT match
rc=1
rc=1
### end

### cksum_posix_crc
printf 'hello\n' | cksum
printf '' | cksum
### expect
3015617425 6
4294967295 0
### end

### base32_and_basenc
printf 'foobar' | base32
printf 'MZXW6YTBOI======' | base32 -d; echo
printf 'foobar' | basenc --base16
printf 'foobar' | basenc --base32hex
printf '\xff\xfe' | basenc --base64url
### expect
MZXW6YTBOI======
foobar
666F6F626172
CPNMUOJ1E8======
__4=
### end

### cmp_reports_first_difference
d=$(mktemp -d); cd "$d"
printf 'hello\n' > a; printf 'hellp\n' > b; printf 'hel' > c; : > e
cmp a a; echo rc=$?
cmp a b; echo rc=$?
cmp -s a b; echo rc=$?
cmp -b a b
cmp a c 2>&1; cmp e a 2>&1
cmp a missing 2>/dev/null; echo rc=$?
### expect
rc=0
a b differ: char 5, line 1
rc=1
rc=1
a b differ: byte 5, line 1 is 157 o 160 p
cmp: EOF on c after byte 3, in line 1
cmp: EOF on e which is empty
rc=2
### end

### factor_numbers
factor 12 97 4294967291 18446744073709551615
echo 1 0 6 | factor
### expect
12: 2 2 3
97: 97
4294967291: 4294967291
18446744073709551615: 3 5 17 257 641 65537 6700417
1:
0:
6: 2 3
### end

### tsort_order_and_loops
printf 'a b\nb c\na d\nd c\ne f\n' | tsort
printf 'a b b a\n' | tsort 2>/dev/null; echo rc=$?
printf 'a b b a\n' | tsort 2>&1 >/dev/null
### expect
a
e
d
b
f
c
a
b
rc=1
tsort: -: input contains a loop:
tsort: a
tsort: b
### end

### nproc_virtual_count
### bash_diff: bashkit reports a fixed virtual CPU count, never the host's
nproc
OMP_NUM_THREADS=2 nproc
nproc --ignore=10
### expect
4
2
1
### end
