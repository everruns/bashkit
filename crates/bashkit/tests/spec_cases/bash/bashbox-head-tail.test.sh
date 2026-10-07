# BashBox head-tail cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_head_tail_head_defaults_to_10_lines
# head defaults to 10 lines
seq 12 | head
### expect
1
2
3
4
5
6
7
8
9
10
### end

### bashbox_head_tail_tail_defaults_to_10_lines
# tail defaults to 10 lines
seq 12 | tail
### expect
3
4
5
6
7
8
9
10
11
12
### end
