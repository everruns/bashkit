# Python environment variable filtering
# Regression tests for issue #999

### readonly_marker_not_visible
# Internal _READONLY_ marker should not be visible in Python
readonly x=1
python3 -c "import os; print(os.getenv('_READONLY_x', 'none'))"
### expect
none
### end

### exported_variable_visible
# Exported variables are visible in Python via os.environ
export MY_VAR=hello
python3 -c "import os; print(os.getenv('MY_VAR', 'missing'))"
### expect
hello
### end

### unexported_variable_not_visible
# Non-exported shell variables are NOT visible in Python (matches bash semantics)
UNEXPORTED_VAR=secret
python3 -c "import os; print(os.getenv('UNEXPORTED_VAR', 'none'))"
### expect
none
### end

### shopt_not_visible
# SHOPT_ variables should not be visible in Python
python3 -c "import os; shopt_vars = [k for k in os.environ if k.startswith('SHOPT_')]; print(len(shopt_vars))"
### expect
0
### end
