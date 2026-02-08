### xxd_plain
### bash_diff: xxd not available in all CI environments
# xxd -p outputs plain hex
printf 'Hi' | xxd -p
### expect
4869
### end

### xxd_basic
### skip: xxd output format varies across platforms
# xxd basic output with offset and ASCII
printf 'AB' | xxd
### expect
00000000: 4142                                     AB
### end

### od_hex
### skip: od output format varies
# od hex output
printf 'AB' | od -t x
### expect
0000000 41 42
0000002
### end

### hexdump_canonical
### skip: hexdump -C output format varies
# hexdump -C canonical display
printf 'Hi' | hexdump -C
### expect
00000000  48 69                                             |Hi|
00000002
### end
