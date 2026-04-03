### bash_source_in_executed_script
# BASH_SOURCE[0] should equal script path when run via bash
echo 'echo "${BASH_SOURCE[0]}"' > /tmp/bsrc_test.sh
bash /tmp/bsrc_test.sh
### expect
/tmp/bsrc_test.sh
### end

### bash_source_dirname_pattern
# Common pattern: find script's own directory
echo 'echo "$(dirname "${BASH_SOURCE[0]}")"' > /tmp/bsrc_dir.sh
bash /tmp/bsrc_dir.sh
### expect
/tmp
### end
