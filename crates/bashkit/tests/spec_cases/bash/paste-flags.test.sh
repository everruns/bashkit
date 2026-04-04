### paste_combined_flags
# paste -sd, should work as -s -d ,
printf "a\nb\nc\n" | paste -sd,
### expect
a,b,c
### end

### paste_combined_flags_tab
# paste -sd with tab delimiter (default-ish)
printf "a\nb\nc\n" | paste -s
### expect
a	b	c
### end

### paste_separate_flags
# paste -s -d , should still work
printf "a\nb\nc\n" | paste -s -d ,
### expect
a,b,c
### end
