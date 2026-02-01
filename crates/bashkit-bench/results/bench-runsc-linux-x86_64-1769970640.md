# BashKit Benchmark Report

## System Information

- **Moniker**: `runsc-linux-x86_64`
- **Hostname**: runsc
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 16
- **Timestamp**: 1769970640
- **Iterations**: 3
- **Warmup**: 1
- **Prewarm cases**: 3

## Summary

Benchmarked 75 cases across 2 runners.

| Runner | Total Time (ms) | Avg/Case (ms) | Errors | Error Rate | Output Match |
|--------|-----------------|---------------|--------|------------|-------------|
| bashkit | 4004.73 | 53.396 | 12 | 5.3% | 80.0% |
| bash | 1663.26 | 22.177 | 0 | 0.0% | 100.0% |

## Performance Comparison

**Bash is 2.4x faster** than bashkit on average.

## Results by Category

### Arithmetic

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arith_basic | bashkit | 0.006 | ±0.001 | - | ✓ |
| arith_basic | bash | 8.585 | ±0.220 | - | ✓ |
| arith_complex | bashkit | 0.006 | ±0.001 | - | ✓ |
| arith_complex | bash | 9.012 | ±0.310 | - | ✓ |
| arith_variables | bashkit | 0.007 | ±0.001 | - | ✓ |
| arith_variables | bash | 8.866 | ±0.136 | - | ✓ |
| arith_increment | bashkit | 1000.000 | ±0.000 | 3 | ✗ |
| arith_increment | bash | 8.813 | ±0.557 | - | ✓ |
| arith_modulo | bashkit | 0.006 | ±0.001 | - | ✓ |
| arith_modulo | bash | 10.796 | ±2.752 | - | ✓ |
| arith_loop_sum | bashkit | 0.020 | ±0.005 | - | ✓ |
| arith_loop_sum | bash | 8.415 | ±0.170 | - | ✓ |

### Arrays

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arr_create | bashkit | 0.006 | ±0.001 | - | ✓ |
| arr_create | bash | 10.190 | ±1.443 | - | ✓ |
| arr_all | bashkit | 0.009 | ±0.002 | - | ✓ |
| arr_all | bash | 10.936 | ±2.625 | - | ✓ |
| arr_length | bashkit | 0.007 | ±0.001 | - | ✓ |
| arr_length | bash | 9.320 | ±0.407 | - | ✓ |
| arr_iterate | bashkit | 0.009 | ±0.002 | - | ✓ |
| arr_iterate | bash | 11.819 | ±2.876 | - | ✓ |
| arr_slice | bashkit | 0.011 | ±0.003 | - | ✗ |
| arr_slice | bash | 10.941 | ±1.991 | - | ✓ |
| arr_assign_index | bashkit | 0.010 | ±0.001 | - | ✓ |
| arr_assign_index | bash | 11.175 | ±1.597 | - | ✓ |

### Complex

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| complex_fibonacci | bashkit | 0.942 | ±0.056 | - | ✓ |
| complex_fibonacci | bash | 593.872 | ±7.341 | - | ✓ |
| complex_fibonacci_iter | bashkit | 0.020 | ±0.002 | - | ✓ |
| complex_fibonacci_iter | bash | 8.513 | ±0.296 | - | ✓ |
| complex_nested_subst | bashkit | 0.010 | ±0.003 | - | ✓ |
| complex_nested_subst | bash | 17.902 | ±0.711 | - | ✓ |
| complex_loop_compute | bashkit | 0.022 | ±0.002 | - | ✓ |
| complex_loop_compute | bash | 8.835 | ±0.516 | - | ✓ |
| complex_string_build | bashkit | 1000.000 | ±0.000 | 3 | ✗ |
| complex_string_build | bash | 8.076 | ±0.242 | - | ✓ |
| complex_json_transform | bashkit | 0.402 | ±0.019 | - | ✓ |
| complex_json_transform | bash | 23.307 | ±0.471 | - | ✓ |
| complex_pipeline_text | bashkit | 0.116 | ±0.028 | - | ✓ |
| complex_pipeline_text | bash | 24.264 | ±0.428 | - | ✓ |

### Control

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| ctrl_if_simple | bashkit | 0.005 | ±0.001 | - | ✓ |
| ctrl_if_simple | bash | 8.665 | ±0.526 | - | ✓ |
| ctrl_if_else | bashkit | 0.006 | ±0.001 | - | ✓ |
| ctrl_if_else | bash | 8.625 | ±0.667 | - | ✓ |
| ctrl_for_list | bashkit | 0.011 | ±0.002 | - | ✓ |
| ctrl_for_list | bash | 8.733 | ±0.236 | - | ✓ |
| ctrl_for_range | bashkit | 1000.000 | ±0.000 | 3 | ✗ |
| ctrl_for_range | bash | 9.949 | ±0.629 | - | ✓ |
| ctrl_while | bashkit | 0.020 | ±0.002 | - | ✓ |
| ctrl_while | bash | 8.691 | ±0.606 | - | ✓ |
| ctrl_case | bashkit | 0.008 | ±0.001 | - | ✓ |
| ctrl_case | bash | 10.308 | ±1.621 | - | ✓ |
| ctrl_function | bashkit | 0.010 | ±0.002 | - | ✓ |
| ctrl_function | bash | 9.347 | ±0.426 | - | ✓ |
| ctrl_function_return | bashkit | 0.011 | ±0.002 | - | ✓ |
| ctrl_function_return | bash | 16.246 | ±3.851 | - | ✓ |
| ctrl_nested_loops | bashkit | 0.017 | ±0.004 | - | ✓ |
| ctrl_nested_loops | bash | 8.810 | ±0.345 | - | ✓ |

### Pipes

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| pipe_simple | bashkit | 0.005 | ±0.001 | - | ✓ |
| pipe_simple | bash | 22.471 | ±0.932 | - | ✓ |
| pipe_multi | bashkit | 0.008 | ±0.001 | - | ✓ |
| pipe_multi | bash | 27.648 | ±2.476 | - | ✓ |
| pipe_command_subst | bashkit | 0.011 | ±0.005 | - | ✓ |
| pipe_command_subst | bash | 11.955 | ±0.197 | - | ✓ |
| pipe_heredoc | bashkit | 0.004 | ±0.001 | - | ✓ |
| pipe_heredoc | bash | 17.136 | ±0.285 | - | ✓ |
| pipe_herestring | bashkit | 0.004 | ±0.001 | - | ✓ |
| pipe_herestring | bash | 17.735 | ±0.235 | - | ✓ |
| pipe_discard | bashkit | 0.007 | ±0.001 | - | ✓ |
| pipe_discard | bash | 11.679 | ±0.357 | - | ✓ |

### Startup

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| startup_empty | bashkit | 0.003 | ±0.000 | - | ✓ |
| startup_empty | bash | 8.521 | ±0.535 | - | ✓ |
| startup_true | bashkit | 0.004 | ±0.001 | - | ✓ |
| startup_true | bash | 9.273 | ±0.554 | - | ✓ |
| startup_echo | bashkit | 0.005 | ±0.000 | - | ✓ |
| startup_echo | bash | 9.954 | ±0.542 | - | ✓ |
| startup_exit | bashkit | 0.003 | ±0.000 | - | ✓ |
| startup_exit | bash | 9.016 | ±0.241 | - | ✓ |

### Strings

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| str_concat | bashkit | 1000.000 | ±0.000 | 3 | ✗ |
| str_concat | bash | 8.770 | ±0.188 | - | ✓ |
| str_printf | bashkit | 0.006 | ±0.001 | - | ✓ |
| str_printf | bash | 8.706 | ±0.234 | - | ✓ |
| str_printf_pad | bashkit | 0.005 | ±0.001 | - | ✗ |
| str_printf_pad | bash | 9.303 | ±0.237 | - | ✓ |
| str_echo_escape | bashkit | 0.005 | ±0.001 | - | ✓ |
| str_echo_escape | bash | 9.578 | ±0.452 | - | ✓ |
| str_prefix_strip | bashkit | 0.006 | ±0.001 | - | ✓ |
| str_prefix_strip | bash | 9.773 | ±0.835 | - | ✓ |
| str_suffix_strip | bashkit | 0.006 | ±0.001 | - | ✓ |
| str_suffix_strip | bash | 10.270 | ±2.176 | - | ✓ |
| str_uppercase | bashkit | 0.005 | ±0.001 | - | ✗ |
| str_uppercase | bash | 8.772 | ±0.612 | - | ✓ |
| str_lowercase | bashkit | 0.006 | ±0.001 | - | ✗ |
| str_lowercase | bash | 8.676 | ±0.381 | - | ✓ |

### Tools

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| tool_grep_simple | bashkit | 0.019 | ±0.009 | - | ✓ |
| tool_grep_simple | bash | 22.690 | ±2.277 | - | ✓ |
| tool_grep_case | bashkit | 0.168 | ±0.111 | - | ✓ |
| tool_grep_case | bash | 23.059 | ±2.619 | - | ✓ |
| tool_grep_count | bashkit | 0.009 | ±0.002 | - | ✓ |
| tool_grep_count | bash | 22.573 | ±0.626 | - | ✓ |
| tool_grep_invert | bashkit | 0.011 | ±0.002 | - | ✓ |
| tool_grep_invert | bash | 20.465 | ±0.239 | - | ✓ |
| tool_grep_regex | bashkit | 0.062 | ±0.030 | - | ✓ |
| tool_grep_regex | bash | 20.646 | ±0.484 | - | ✓ |
| tool_sed_replace | bashkit | 0.238 | ±0.181 | - | ✓ |
| tool_sed_replace | bash | 26.791 | ±0.781 | - | ✓ |
| tool_sed_global | bashkit | 0.078 | ±0.010 | - | ✓ |
| tool_sed_global | bash | 23.593 | ±2.095 | - | ✓ |
| tool_sed_delete | bashkit | 0.009 | ±0.002 | - | ✓ |
| tool_sed_delete | bash | 21.328 | ±0.168 | - | ✓ |
| tool_sed_lines | bashkit | 0.006 | ±0.001 | - | ✓ |
| tool_sed_lines | bash | 21.269 | ±0.758 | - | ✓ |
| tool_sed_backrefs | bashkit | 0.138 | ±0.008 | - | ✗ |
| tool_sed_backrefs | bash | 20.960 | ±0.539 | - | ✓ |
| tool_awk_print | bashkit | 0.009 | ±0.002 | - | ✓ |
| tool_awk_print | bash | 21.516 | ±2.283 | - | ✓ |
| tool_awk_sum | bashkit | 0.015 | ±0.003 | - | ✓ |
| tool_awk_sum | bash | 24.404 | ±2.135 | - | ✓ |
| tool_awk_pattern | bashkit | 0.028 | ±0.004 | - | ✓ |
| tool_awk_pattern | bash | 20.525 | ±0.405 | - | ✓ |
| tool_awk_fieldsep | bashkit | 0.010 | ±0.001 | - | ✓ |
| tool_awk_fieldsep | bash | 21.460 | ±0.424 | - | ✓ |
| tool_awk_nf | bashkit | 0.008 | ±0.002 | - | ✓ |
| tool_awk_nf | bash | 21.262 | ±1.031 | - | ✓ |
| tool_awk_compute | bashkit | 0.015 | ±0.003 | - | ✓ |
| tool_awk_compute | bash | 22.074 | ±2.493 | - | ✓ |
| tool_jq_identity | bashkit | 0.491 | ±0.059 | - | ✗ |
| tool_jq_identity | bash | 23.262 | ±0.358 | - | ✓ |
| tool_jq_field | bashkit | 0.417 | ±0.018 | - | ✓ |
| tool_jq_field | bash | 25.878 | ±2.325 | - | ✓ |
| tool_jq_array | bashkit | 0.371 | ±0.003 | - | ✓ |
| tool_jq_array | bash | 24.934 | ±3.126 | - | ✓ |
| tool_jq_filter | bashkit | 0.388 | ±0.028 | - | ✗ |
| tool_jq_filter | bash | 24.072 | ±1.305 | - | ✓ |
| tool_jq_map | bashkit | 0.395 | ±0.025 | - | ✗ |
| tool_jq_map | bash | 25.708 | ±2.312 | - | ✓ |

### Variables

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| var_assign_simple | bashkit | 0.005 | ±0.001 | - | ✓ |
| var_assign_simple | bash | 9.200 | ±0.656 | - | ✓ |
| var_assign_many | bashkit | 0.014 | ±0.001 | - | ✓ |
| var_assign_many | bash | 8.545 | ±0.196 | - | ✓ |
| var_default | bashkit | 0.004 | ±0.001 | - | ✓ |
| var_default | bash | 8.407 | ±0.219 | - | ✓ |
| var_length | bashkit | 0.005 | ±0.001 | - | ✓ |
| var_length | bash | 8.441 | ±0.274 | - | ✓ |
| var_substring | bashkit | 0.006 | ±0.001 | - | ✗ |
| var_substring | bash | 8.705 | ±0.190 | - | ✓ |
| var_replace | bashkit | 0.006 | ±0.001 | - | ✗ |
| var_replace | bash | 10.284 | ±2.218 | - | ✓ |
| var_nested | bashkit | 0.007 | ±0.001 | - | ✗ |
| var_nested | bash | 10.358 | ±1.814 | - | ✓ |
| var_export | bashkit | 0.005 | ±0.001 | - | ✓ |
| var_export | bash | 8.603 | ±0.058 | - | ✓ |

## Assumptions & Notes

- Times measured in nanoseconds, displayed in milliseconds
- Prewarm phase runs first few cases to warm up JIT/compilation
- Per-benchmark warmup iterations excluded from timing
- Output match compares against bash output when available
- Errors include execution failures and exit code mismatches
- BashKit runs in-process (no fork), bash spawns subprocess

