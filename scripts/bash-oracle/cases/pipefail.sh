false | true; echo "sem pipefail: $?"
set -o pipefail
false | true; echo "com pipefail: $?"
(exit 3) | (exit 4) | true; echo "último não-zero: $?"
yes | head -n 1; echo "sigpipe com pipefail: $? ${PIPESTATUS[*]}"
set +o pipefail
shopt -o pipefail
