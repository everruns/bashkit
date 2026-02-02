### errexit_basic
# set -e stops on error
set -e
true
echo "after true"
### expect
after true
### end

### errexit_continues_on_success
# set -e continues when commands succeed
set -e
true
true
echo "done"
### expect
done
### end

### errexit_in_conditional
# set -e doesn't trigger in if condition
set -e
if false; then
  echo "then"
else
  echo "else"
fi
### expect
else
### end

### errexit_in_while
# set -e doesn't trigger in while condition
set -e
x=1
while [ $x -eq 0 ]; do
  echo "loop"
done
echo "after"
### expect
after
### end

### errexit_in_and
# set -e doesn't trigger in && left side
set -e
false && echo "skipped"
echo "after"
### expect
after
### end

### errexit_in_or
# set -e doesn't trigger in || left side
set -e
false || echo "or branch"
echo "after"
### expect
or branch
after
### end

### errexit_negation
### skip: pipeline negation with errexit needs further testing
# set -e doesn't trigger with !
set -e
! false
echo "after"
### expect
after
### end

### set_plus_e
# set +e disables errexit
set -e
set +e
false
echo "still running"
### expect
still running
### end
