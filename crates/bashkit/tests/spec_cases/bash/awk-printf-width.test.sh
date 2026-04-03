### awk_printf_huge_width_rejected
# printf with enormous width should error, not OOM
echo "" | awk '{printf "%999999999d", 1}' 2>&1
echo "exit: $?"
### expect
awk: format width 999999999 exceeds maximum (10000)
exit: 2
### end

### awk_printf_normal_width_works
# Normal width should still work
echo "" | awk '{printf "%20d\n", 42}'
### expect
                  42
### end

### awk_printf_max_width_works
# Width at limit boundary should work
echo "" | awk '{printf "%10000d\n", 1}' | wc -c
### expect
10001
### end

### awk_printf_huge_precision_rejected
# Huge precision should also error
echo "" | awk '{printf "%.999999999f", 1}' 2>&1
echo "exit: $?"
### expect
awk: format precision 999999999 exceeds maximum (10000)
exit: 2
### end
