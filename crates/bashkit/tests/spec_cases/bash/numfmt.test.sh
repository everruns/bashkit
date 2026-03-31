### numfmt_to_iec
numfmt --to=iec 1048576
### expect
1.0M
### end

### numfmt_to_si
numfmt --to=si 1048576
### expect
1.1M
### end

### numfmt_from_iec
numfmt --from=iec 1K
### expect
1024
### end

### numfmt_from_si
numfmt --from=si 1K
### expect
1000
### end

### numfmt_to_iec_suffix
numfmt --to=iec --suffix=B 1048576
### expect
1.0MB
### end

### numfmt_to_iec_i
numfmt --to=iec-i 1048576
### expect
1.0Mi
### end

### numfmt_roundtrip_iec
numfmt --from=iec --to=iec 1M
### expect
1.0M
### end

### numfmt_stdin
echo 1048576 | numfmt --to=iec
### expect
1.0M
### end

### numfmt_multiple_args
numfmt --to=iec 1024 2048 1048576
### expect
1.0K
2.0K
1.0M
### end

### numfmt_padding
numfmt --to=iec --padding=10 1048576
### expect
      1.0M
### end

### numfmt_small_number
numfmt --to=iec 500
### expect
500
### end

### numfmt_large_number
numfmt --to=si 1000000000
### expect
1.0G
### end

### numfmt_invalid_number
numfmt --to=iec abc
echo "exit: $?"
### expect
exit: 2
### end
