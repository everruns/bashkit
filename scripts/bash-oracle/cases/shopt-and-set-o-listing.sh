shopt extglob nullglob globstar
set -o | grep -E '^(errexit|nounset|pipefail|noclobber)'
shopt -q extglob; echo "extglob q=$?"
