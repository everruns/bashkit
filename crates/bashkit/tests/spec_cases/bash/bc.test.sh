### bc_basic_addition
# bc basic addition
echo "1+2" | bc
### expect
3
### end

### bc_subtraction
# bc subtraction
echo "10-3" | bc
### expect
7
### end

### bc_multiplication
# bc multiplication
echo "6*7" | bc
### expect
42
### end

### bc_division_integer
# bc integer division (scale=0)
echo "10/3" | bc
### expect
3
### end

### bc_scale_division
# bc with scale for decimal division
echo "scale=2; 10/3" | bc
### expect
3.33
### end

### bc_power
# bc exponentiation
echo "2^10" | bc
### expect
1024
### end

### bc_parentheses
# bc with parentheses
echo "(2+3)*4" | bc
### expect
20
### end

### bc_financial
### bash_diff: VFS bc applies scale to all operations; real bc scale only affects division
# bc financial calculation with scale
echo "scale=2; 100.50 * 1.0825" | bc
### expect
108.79
### end

### bc_multiple_expressions
# bc handles multiple expressions
printf "1+1\n2+2\n3+3\n" | bc
### expect
2
4
6
### end

### bc_negative
# bc negative numbers
echo "-5+3" | bc
### expect
-2
### end

### bc_modulo
# bc modulo
echo "10%3" | bc
### expect
1
### end

### bc_variable
# bc variable assignment and use
printf "x=5\nx*2\n" | bc
### expect
10
### end

### bc_comparison
# bc comparison operators
echo "5==5" | bc
### expect
1
### end

### bc_sqrt
# bc sqrt function
echo "scale=4; sqrt(2)" | bc
### expect
1.4142
### end

### bc_divide_by_zero
### bash_diff: VFS bc returns exit 1 on divide by zero; real bc returns exit 0 with stderr
### exit_code: 1
# bc divide by zero error
echo "1/0" | bc
### expect
### end
