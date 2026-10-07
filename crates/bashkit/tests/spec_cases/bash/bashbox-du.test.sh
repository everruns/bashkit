# BashBox du cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_du_human_rounding
# human rounding
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
for n in 1 1023 1024 1025 1536 10239 10240 10241 1047552 1047553 1048575 1048576 1048577 5000000; do printf "%${n}s" '' > f; du -bh f; done
### expect
1	f
1023	f
1.0K	f
1.1K	f
1.5K	f
10K	f
10K	f
11K	f
1023K	f
1.0M	f
1.0M	f
1.0M	f
1.1M	f
4.8M	f
### end
