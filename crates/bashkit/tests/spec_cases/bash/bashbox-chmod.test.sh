# BashBox chmod cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_chmod_f_stays_quiet_but_still_fails
# -f stays quiet but still fails
chmod -f 644 nope; ln -s nope dl; chmod -f 644 dl
### expect
### end
