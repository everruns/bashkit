### test_t_default_false
# -t 0 defaults to false in VFS sandbox
if [ -t 0 ]; then
  echo "terminal"
else
  echo "not terminal"
fi
### expect
not terminal
### end

### test_t_stdout_false
# -t 1 defaults to false
if [ -t 1 ]; then
  echo "terminal"
else
  echo "not terminal"
fi
### expect
not terminal
### end

### test_t_configurable
### bash_diff: _TTY_N is a bashkit-specific extension for configuring terminal state
# _TTY_1=1 makes -t 1 return true
_TTY_1=1
if [ -t 1 ]; then
  echo "terminal"
else
  echo "not terminal"
fi
### expect
terminal
### end

### test_t_conditional_syntax
# [[ -t 0 ]] also works
if [[ -t 0 ]]; then
  echo "terminal"
else
  echo "not terminal"
fi
### expect
not terminal
### end

### test_t_conditional_configurable
### bash_diff: _TTY_N is a bashkit-specific extension for configuring terminal state
# [[ -t 1 ]] respects _TTY_1 variable
_TTY_1=1
if [[ -t 1 ]]; then
  echo "terminal"
else
  echo "not terminal"
fi
### expect
terminal
### end
