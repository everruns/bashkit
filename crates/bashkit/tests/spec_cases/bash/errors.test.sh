### syntax_unclosed_paren
# Unclosed parenthesis should error
### skip: causes parser hang - needs timeout fix
### expect_error
echo $(echo
### expect
### end

### syntax_unclosed_quote
# Unclosed quote should error
### skip: causes parser hang - needs timeout fix
### expect_error
echo "hello
### expect
### end

### syntax_unclosed_brace
# Unclosed brace should error
### skip: causes parser hang - needs timeout fix
### expect_error
echo ${var
### expect
### end

### undefined_variable_empty
# Undefined variable expands to empty string
FOO=$UNDEFINED_VAR_XYZ
echo "value:$FOO:"
### expect
value::
### end

### division_by_zero
# Division by zero behavior
### skip: division by zero handling varies
### expect_error
echo $((1/0))
### expect
### end

### empty_command
# Empty command is valid (no-op)
### skip: leading semicolon parsing issue
;
echo done
### expect
done
### end

### command_not_found
# Unknown command should have exit code
### skip: external command handling not implemented
nonexistent_command_xyz_123
### exit_code: 127
### expect
### end

### bad_substitution
# Invalid parameter expansion
### skip: causes parser hang
### expect_error
echo ${!}
### expect
### end

### arithmetic_syntax_error
# Invalid arithmetic expression
### skip: causes hang
### expect_error
echo $((1 + + 2))
### expect
### end

### unmatched_fi
# fi without if should error
### skip: causes parser hang
### expect_error
fi
### expect
### end

### unmatched_done
# done without for/while should error
### skip: causes parser hang
### expect_error
done
### expect
### end

### redirect_no_target
# Redirect without target should error
### skip: causes parser hang
### expect_error
echo hello >
### expect
### end
