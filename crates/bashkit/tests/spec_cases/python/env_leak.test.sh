# Python environment variable filtering
# Regression tests for issue #999

### readonly_marker_not_visible
# Internal _READONLY_ marker should not be visible in Python
readonly x=1
python3 -c "import os; print(os.getenv('_READONLY_x', 'none'))"
### expect
none
### end

### user_variable_still_visible
# Regular user variables should still be accessible from Python
MY_VAR=hello
python3 -c "import os; print(os.getenv('MY_VAR', 'missing'))"
### expect
hello
### end

### shopt_not_visible
# SHOPT_ variables should not be visible in Python
python3 -c "import os; shopt_vars = [k for k in os.environ if k.startswith('SHOPT_')]; print(len(shopt_vars))"
### expect
0
### end
