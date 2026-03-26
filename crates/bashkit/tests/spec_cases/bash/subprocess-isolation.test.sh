### subprocess_non_exported_vars_not_visible
# Non-exported variables should not be visible in child scripts
local_var="secret"
export PUBLIC_VAR="visible"

cat > /tmp/check.sh <<'SCRIPT'
#!/usr/bin/env bash
echo "public=${PUBLIC_VAR:-unset}"
echo "local=${local_var:-unset}"
SCRIPT
chmod +x /tmp/check.sh

/tmp/check.sh
### expect
public=visible
local=unset
### end

### subprocess_child_changes_dont_affect_parent
# Variable changes in child script don't propagate to parent
export COUNTER=0

cat > /tmp/increment.sh <<'SCRIPT'
#!/usr/bin/env bash
COUNTER=$((COUNTER + 1))
echo "child: ${COUNTER}"
SCRIPT
chmod +x /tmp/increment.sh

/tmp/increment.sh
echo "parent: ${COUNTER}"
### expect
child: 1
parent: 0
### end

### subprocess_functions_not_inherited
# Functions defined in parent are not visible in child scripts
helper() { echo "from parent"; }

cat > /tmp/call-helper.sh <<'SCRIPT'
#!/usr/bin/env bash
helper 2>/dev/null
echo "exit=$?"
SCRIPT
chmod +x /tmp/call-helper.sh

/tmp/call-helper.sh
### expect
exit=127
### end

### subprocess_non_exported_arrays_not_visible
# Non-exported arrays should not be visible in child scripts
my_array=(one two three)
export SIMPLE="yes"

cat > /tmp/check-array.sh <<'SCRIPT'
#!/usr/bin/env bash
echo "simple=${SIMPLE:-unset}"
echo "array_len=${#my_array[@]}"
SCRIPT
chmod +x /tmp/check-array.sh

/tmp/check-array.sh
### expect
simple=yes
array_len=0
### end

### subprocess_source_shares_state
# source/. should still share full parent state (not isolated)
local_var="visible-to-source"

cat > /tmp/sourced.sh <<'SCRIPT'
echo "local=${local_var:-unset}"
local_var="modified"
SCRIPT

source /tmp/sourced.sh
echo "after source: ${local_var}"
### expect
local=visible-to-source
after source: modified
### end

### subprocess_exit_code_propagation
# Exit code from child script propagates correctly
cat > /tmp/fail.sh <<'SCRIPT'
#!/usr/bin/env bash
exit 42
SCRIPT
chmod +x /tmp/fail.sh

/tmp/fail.sh
echo "exit: $?"
### expect
exit: 42
### end

### subprocess_exported_vars_survive_nesting
# Exported variables survive through nested script execution
export LEVEL="parent"

cat > /tmp/outer.sh <<'SCRIPT'
#!/usr/bin/env bash
echo "outer sees: ${LEVEL}"
export LEVEL="outer"
cat > /tmp/inner.sh <<'INNERSCRIPT'
#!/usr/bin/env bash
echo "inner sees: ${LEVEL}"
INNERSCRIPT
chmod +x /tmp/inner.sh
/tmp/inner.sh
SCRIPT
chmod +x /tmp/outer.sh

/tmp/outer.sh
echo "parent still: ${LEVEL}"
### expect
outer sees: parent
inner sees: outer
parent still: parent
### end

### subprocess_path_search_isolated
# PATH-based script execution should also be isolated
not_exported="hidden"
export VISIBLE="yes"

mkdir -p /usr/local/bin
cat > /usr/local/bin/check-isolation <<'SCRIPT'
#!/usr/bin/env bash
echo "visible=${VISIBLE:-unset}"
echo "hidden=${not_exported:-unset}"
SCRIPT
chmod +x /usr/local/bin/check-isolation

PATH="/usr/local/bin:${PATH}" check-isolation
### expect
visible=yes
hidden=unset
### end
