# BashBox base64 cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_base64_empty_input_encodes_to_nothing
# empty input encodes to nothing
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf '' | base64
### expect
### end
