# BashBox temp-path cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_temp_path_quiet_hides_creation_failures
# --quiet hides creation failures
mktemp --quiet -p /nonexist fooXXX
### expect
### end
