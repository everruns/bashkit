# BashKit Benchmark Report

## System Information

- **Moniker**: `runsc-linux-x86_64`
- **Hostname**: runsc
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 16
- **Timestamp**: 1770091273
- **Iterations**: 10
- **Warmup**: 2
- **Prewarm cases**: 3

## Summary

Benchmarked 75 cases across 2 runners.

| Runner | Total Time (ms) | Avg/Case (ms) | Errors | Error Rate | Output Match |
|--------|-----------------|---------------|--------|------------|-------------|
| bashkit | 9.96 | 0.133 | 0 | 0.0% | 89.3% |
| bash | 2129.16 | 28.389 | 0 | 0.0% | 100.0% |

## Performance Comparison

**BashKit is 213.7x faster** than bash on average.

## Results by Category

### Arithmetic

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arith_basic | bashkit | 0.045 | ±0.030 | - | ✓ |
| arith_basic | bash | 10.308 | ±1.531 | - | ✓ |
| arith_complex | bashkit | 0.067 | ±0.020 | - | ✓ |
| arith_complex | bash | 10.149 | ±1.566 | - | ✓ |
| arith_variables | bashkit | 0.065 | ±0.023 | - | ✓ |
| arith_variables | bash | 9.744 | ±0.648 | - | ✓ |
| arith_increment | bashkit | 0.075 | ±0.026 | - | ✓ |
| arith_increment | bash | 10.231 | ±1.424 | - | ✓ |
| arith_modulo | bashkit | 0.054 | ±0.020 | - | ✓ |
| arith_modulo | bash | 11.093 | ±1.785 | - | ✓ |
| arith_loop_sum | bashkit | 0.073 | ±0.024 | - | ✓ |
| arith_loop_sum | bash | 10.358 | ±1.936 | - | ✓ |

### Arrays

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arr_create | bashkit | 0.064 | ±0.028 | - | ✓ |
| arr_create | bash | 11.107 | ±1.042 | - | ✓ |
| arr_all | bashkit | 0.048 | ±0.029 | - | ✓ |
| arr_all | bash | 10.707 | ±0.520 | - | ✓ |
| arr_length | bashkit | 0.061 | ±0.026 | - | ✓ |
| arr_length | bash | 12.235 | ±1.670 | - | ✓ |
| arr_iterate | bashkit | 0.074 | ±0.047 | - | ✓ |
| arr_iterate | bash | 10.992 | ±0.806 | - | ✓ |
| arr_slice | bashkit | 0.074 | ±0.024 | - | ✗ |
| arr_slice | bash | 10.877 | ±0.934 | - | ✓ |
| arr_assign_index | bashkit | 0.066 | ±0.028 | - | ✓ |
| arr_assign_index | bash | 11.417 | ±0.817 | - | ✓ |

### Complex

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| complex_fibonacci | bashkit | 1.617 | ±0.384 | - | ✓ |
| complex_fibonacci | bash | 831.255 | ±38.293 | - | ✓ |
| complex_fibonacci_iter | bashkit | 0.079 | ±0.017 | - | ✓ |
| complex_fibonacci_iter | bash | 10.064 | ±0.499 | - | ✓ |
| complex_nested_subst | bashkit | 0.084 | ±0.037 | - | ✓ |
| complex_nested_subst | bash | 23.542 | ±2.629 | - | ✓ |
| complex_loop_compute | bashkit | 0.102 | ±0.050 | - | ✓ |
| complex_loop_compute | bash | 10.602 | ±0.365 | - | ✓ |
| complex_string_build | bashkit | 0.108 | ±0.093 | - | ✓ |
| complex_string_build | bash | 11.714 | ±2.980 | - | ✓ |
| complex_json_transform | bashkit | 0.441 | ±0.028 | - | ✓ |
| complex_json_transform | bash | 32.423 | ±2.268 | - | ✓ |
| complex_pipeline_text | bashkit | 0.320 | ±0.117 | - | ✓ |
| complex_pipeline_text | bash | 34.493 | ±2.603 | - | ✓ |

### Control

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| ctrl_if_simple | bashkit | 0.048 | ±0.027 | - | ✓ |
| ctrl_if_simple | bash | 9.929 | ±0.556 | - | ✓ |
| ctrl_if_else | bashkit | 0.055 | ±0.019 | - | ✓ |
| ctrl_if_else | bash | 11.105 | ±2.192 | - | ✓ |
| ctrl_for_list | bashkit | 0.076 | ±0.028 | - | ✓ |
| ctrl_for_list | bash | 9.464 | ±0.362 | - | ✓ |
| ctrl_for_range | bashkit | 0.082 | ±0.029 | - | ✓ |
| ctrl_for_range | bash | 11.444 | ±2.095 | - | ✓ |
| ctrl_while | bashkit | 0.088 | ±0.028 | - | ✓ |
| ctrl_while | bash | 10.564 | ±0.656 | - | ✓ |
| ctrl_case | bashkit | 0.074 | ±0.015 | - | ✓ |
| ctrl_case | bash | 10.278 | ±0.362 | - | ✓ |
| ctrl_function | bashkit | 0.078 | ±0.031 | - | ✓ |
| ctrl_function | bash | 11.229 | ±1.349 | - | ✓ |
| ctrl_function_return | bashkit | 0.093 | ±0.033 | - | ✓ |
| ctrl_function_return | bash | 14.412 | ±0.684 | - | ✓ |
| ctrl_nested_loops | bashkit | 0.072 | ±0.032 | - | ✓ |
| ctrl_nested_loops | bash | 10.235 | ±0.417 | - | ✓ |

### Pipes

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| pipe_simple | bashkit | 0.039 | ±0.021 | - | ✓ |
| pipe_simple | bash | 26.293 | ±1.218 | - | ✓ |
| pipe_multi | bashkit | 0.067 | ±0.038 | - | ✓ |
| pipe_multi | bash | 32.057 | ±2.366 | - | ✓ |
| pipe_command_subst | bashkit | 0.059 | ±0.025 | - | ✓ |
| pipe_command_subst | bash | 14.731 | ±1.135 | - | ✓ |
| pipe_heredoc | bashkit | 0.053 | ±0.021 | - | ✓ |
| pipe_heredoc | bash | 24.036 | ±1.787 | - | ✓ |
| pipe_herestring | bashkit | 0.052 | ±0.026 | - | ✓ |
| pipe_herestring | bash | 23.330 | ±1.236 | - | ✓ |
| pipe_discard | bashkit | 0.067 | ±0.033 | - | ✓ |
| pipe_discard | bash | 15.197 | ±0.553 | - | ✓ |

### Startup

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| startup_empty | bashkit | 0.089 | ±0.090 | - | ✓ |
| startup_empty | bash | 10.053 | ±0.767 | - | ✓ |
| startup_true | bashkit | 0.042 | ±0.023 | - | ✓ |
| startup_true | bash | 9.703 | ±0.390 | - | ✓ |
| startup_echo | bashkit | 0.053 | ±0.026 | - | ✓ |
| startup_echo | bash | 10.776 | ±1.719 | - | ✓ |
| startup_exit | bashkit | 0.061 | ±0.015 | - | ✓ |
| startup_exit | bash | 10.523 | ±1.656 | - | ✓ |

### Strings

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| str_concat | bashkit | 0.074 | ±0.037 | - | ✓ |
| str_concat | bash | 10.764 | ±1.657 | - | ✓ |
| str_printf | bashkit | 0.059 | ±0.023 | - | ✓ |
| str_printf | bash | 9.901 | ±0.435 | - | ✓ |
| str_printf_pad | bashkit | 0.057 | ±0.029 | - | ✗ |
| str_printf_pad | bash | 10.949 | ±0.729 | - | ✓ |
| str_echo_escape | bashkit | 0.052 | ±0.020 | - | ✓ |
| str_echo_escape | bash | 11.568 | ±1.872 | - | ✓ |
| str_prefix_strip | bashkit | 0.069 | ±0.036 | - | ✓ |
| str_prefix_strip | bash | 11.789 | ±2.150 | - | ✓ |
| str_suffix_strip | bashkit | 0.069 | ±0.023 | - | ✓ |
| str_suffix_strip | bash | 10.791 | ±0.637 | - | ✓ |
| str_uppercase | bashkit | 0.053 | ±0.029 | - | ✗ |
| str_uppercase | bash | 10.942 | ±0.839 | - | ✓ |
| str_lowercase | bashkit | 0.062 | ±0.032 | - | ✗ |
| str_lowercase | bash | 10.603 | ±0.557 | - | ✓ |

### Tools

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| tool_grep_simple | bashkit | 0.044 | ±0.017 | - | ✓ |
| tool_grep_simple | bash | 27.472 | ±1.200 | - | ✓ |
| tool_grep_case | bashkit | 0.214 | ±0.126 | - | ✓ |
| tool_grep_case | bash | 26.275 | ±0.884 | - | ✓ |
| tool_grep_count | bashkit | 0.083 | ±0.036 | - | ✓ |
| tool_grep_count | bash | 26.093 | ±1.207 | - | ✓ |
| tool_grep_invert | bashkit | 0.063 | ±0.038 | - | ✓ |
| tool_grep_invert | bash | 27.113 | ±1.843 | - | ✓ |
| tool_grep_regex | bashkit | 0.122 | ±0.033 | - | ✓ |
| tool_grep_regex | bash | 26.280 | ±1.762 | - | ✓ |
| tool_sed_replace | bashkit | 0.165 | ±0.028 | - | ✓ |
| tool_sed_replace | bash | 29.206 | ±1.281 | - | ✓ |
| tool_sed_global | bashkit | 0.130 | ±0.012 | - | ✓ |
| tool_sed_global | bash | 28.601 | ±1.523 | - | ✓ |
| tool_sed_delete | bashkit | 0.070 | ±0.037 | - | ✓ |
| tool_sed_delete | bash | 29.461 | ±2.065 | - | ✓ |
| tool_sed_lines | bashkit | 0.071 | ±0.033 | - | ✓ |
| tool_sed_lines | bash | 28.224 | ±1.486 | - | ✓ |
| tool_sed_backrefs | bashkit | 0.250 | ±0.096 | - | ✗ |
| tool_sed_backrefs | bash | 28.910 | ±2.036 | - | ✓ |
| tool_awk_print | bashkit | 0.056 | ±0.021 | - | ✓ |
| tool_awk_print | bash | 27.753 | ±1.760 | - | ✓ |
| tool_awk_sum | bashkit | 0.068 | ±0.028 | - | ✓ |
| tool_awk_sum | bash | 26.951 | ±1.386 | - | ✓ |
| tool_awk_pattern | bashkit | 0.079 | ±0.035 | - | ✓ |
| tool_awk_pattern | bash | 27.180 | ±2.571 | - | ✓ |
| tool_awk_fieldsep | bashkit | 0.064 | ±0.034 | - | ✓ |
| tool_awk_fieldsep | bash | 28.611 | ±2.004 | - | ✓ |
| tool_awk_nf | bashkit | 0.076 | ±0.048 | - | ✓ |
| tool_awk_nf | bash | 27.694 | ±2.101 | - | ✓ |
| tool_awk_compute | bashkit | 0.093 | ±0.048 | - | ✓ |
| tool_awk_compute | bash | 27.982 | ±1.973 | - | ✓ |
| tool_jq_identity | bashkit | 0.532 | ±0.087 | - | ✓ |
| tool_jq_identity | bash | 30.133 | ±2.021 | - | ✓ |
| tool_jq_field | bashkit | 0.569 | ±0.144 | - | ✓ |
| tool_jq_field | bash | 30.306 | ±0.737 | - | ✓ |
| tool_jq_array | bashkit | 0.447 | ±0.020 | - | ✓ |
| tool_jq_array | bash | 31.935 | ±3.847 | - | ✓ |
| tool_jq_filter | bashkit | 0.479 | ±0.020 | - | ✓ |
| tool_jq_filter | bash | 31.553 | ±1.398 | - | ✓ |
| tool_jq_map | bashkit | 0.523 | ±0.088 | - | ✓ |
| tool_jq_map | bash | 31.600 | ±1.368 | - | ✓ |

### Variables

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| var_assign_simple | bashkit | 0.043 | ±0.021 | - | ✓ |
| var_assign_simple | bash | 10.432 | ±0.622 | - | ✓ |
| var_assign_many | bashkit | 0.091 | ±0.050 | - | ✓ |
| var_assign_many | bash | 10.610 | ±1.603 | - | ✓ |
| var_default | bashkit | 0.043 | ±0.023 | - | ✓ |
| var_default | bash | 10.497 | ±1.362 | - | ✓ |
| var_length | bashkit | 0.070 | ±0.033 | - | ✓ |
| var_length | bash | 10.676 | ±1.325 | - | ✓ |
| var_substring | bashkit | 0.081 | ±0.052 | - | ✗ |
| var_substring | bash | 10.151 | ±1.079 | - | ✓ |
| var_replace | bashkit | 0.062 | ±0.027 | - | ✗ |
| var_replace | bash | 10.046 | ±0.434 | - | ✓ |
| var_nested | bashkit | 0.060 | ±0.036 | - | ✗ |
| var_nested | bash | 10.950 | ±1.299 | - | ✓ |
| var_export | bashkit | 0.057 | ±0.028 | - | ✓ |
| var_export | bash | 10.493 | ±1.011 | - | ✓ |

## Assumptions & Notes

- Times measured in nanoseconds, displayed in milliseconds
- Prewarm phase runs first few cases to warm up JIT/compilation
- Per-benchmark warmup iterations excluded from timing
- Output match compares against bash output when available
- Errors include execution failures and exit code mismatches
- BashKit runs in-process (no fork), bash spawns subprocess

