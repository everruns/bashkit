# BashBox filesystem-backends-integration cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_filesystem_backends_integration_a_file_that_may_not_be_created
# a file that may not be created
echo x > ro/new; echo $?
### expect
1
### end
