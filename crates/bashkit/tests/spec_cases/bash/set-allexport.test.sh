### set_a_basic
# set -a exports new variables to env
set -a
FOO="bar"
BAZ="qux"
set +a
AFTER="not-exported"
env | grep -c "^FOO=bar$"
env | grep -c "^BAZ=qux$"
env | grep -c "^AFTER="
### expect
1
1
0
### end

### set_o_allexport
# set -o allexport / set +o allexport works
set -o allexport
X="hello"
set +o allexport
Y="world"
env | grep -c "^X=hello$"
env | grep -c "^Y="
### expect
1
0
### end

### set_a_not_retroactive
# Variables assigned before set -a are not retroactively exported
BEFORE="exists"
set -a
DURING="new"
set +a
env | grep -c "^BEFORE="
env | grep -c "^DURING=new$"
### expect
0
1
### end

### set_a_export_p
# export -p lists allexported variables
set -a
EXPVAR="test123"
set +a
export -p | grep -c "declare -x EXPVAR="
### expect
1
### end

### set_a_source
# set -a with source exports sourced variables
cat > /tmp/vars.env <<'EOF'
DB_HOST=localhost
DB_PORT=5432
EOF
set -a
source /tmp/vars.env
set +a
env | grep -c "^DB_HOST=localhost$"
env | grep -c "^DB_PORT=5432$"
### expect
1
1
### end
