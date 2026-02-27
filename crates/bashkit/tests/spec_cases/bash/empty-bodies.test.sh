# Empty body tests
# Inspired by Oils spec/empty-bodies.test.sh
# https://github.com/oilshell/oil/blob/master/spec/empty-bodies.test.sh

### empty_case_esac
# Empty case/esac is valid
case foo in
esac
echo empty
### expect
empty
### end

### empty_while_do_done
# Empty while body - bash treats as parse error, bashkit allows it
### skip: TODO empty while body not rejected as parse error
bash -c 'while false; do
done
echo empty' 2>/dev/null
echo status=$?
### expect
status=2
### end

### empty_if_then_fi
# Empty then body - bash treats as parse error, bashkit allows it
### skip: TODO empty if body not rejected as parse error
bash -c 'if true; then
fi
echo empty' 2>/dev/null
echo status=$?
### expect
status=2
### end

### empty_else_clause
# Empty else clause - bash treats as parse error, bashkit allows it
### skip: TODO empty else body not rejected as parse error
bash -c 'if false; then echo yes; else
fi' 2>/dev/null
echo status=$?
### expect
status=2
### end

### empty_for_body
# Empty for body - bash treats as parse error, bashkit allows it
### skip: TODO empty for body not rejected as parse error
bash -c 'for i in 1 2 3; do
done' 2>/dev/null
echo status=$?
### expect
status=2
### end

### empty_function_body
# Empty function body - bash treats as parse error, bashkit allows it
### skip: TODO empty function body not rejected as parse error
bash -c 'f() { }' 2>/dev/null
echo status=$?
### expect
status=2
### end

### case_with_empty_clause
# case with empty clauses is valid
case foo in
  bar)
    ;;
  foo)
    echo matched
    ;;
esac
### expect
matched
### end

### case_empty_fallthrough
# case with empty clause and fallthrough
case x in
  x)
    ;;
  *)
    echo no
    ;;
esac
echo done
### expect
done
### end
