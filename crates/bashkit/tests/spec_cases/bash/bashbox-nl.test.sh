# BashBox nl cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_nl_one_char_delimiter
# one-char delimiter
printf '@:@:\nh\n@:\nb\n' | nl -d @ -ha
### expect

     1	h

       b
### end

### bashbox_nl_three_char_delimiter
# three-char delimiter
printf 'abcabc\nh\nabc\nf\n' | nl -d abc -ha -fa
### expect

     1	h

     1	f
### end

### bashbox_nl_line_with_delimiter_and_more_text_is_a_body_line
# line with delimiter and more text is a body line
printf '\\:x\n' | nl
### expect
     1	\:x
### end

### bashbox_nl_join_counts_across_sections
# join counts across sections
printf 'a\n\n\\:\:\n\nb\n' | nl -ba -l2
### expect
     1	a
       

     1	
     2	b
### end
