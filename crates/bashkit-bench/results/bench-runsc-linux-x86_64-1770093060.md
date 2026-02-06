# Bashkit Benchmark Report

## System Information

- **Moniker**: `runsc-linux-x86_64`
- **Hostname**: runsc
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 16
- **Timestamp**: 1770093060
- **Iterations**: 10
- **Warmup**: 2
- **Prewarm cases**: 3

## Summary

Benchmarked 75 cases across 2 runners.

| Runner | Total Time (ms) | Avg/Case (ms) | Errors | Error Rate | Output Match |
|--------|-----------------|---------------|--------|------------|-------------|
| bashkit | 8.97 | 0.120 | 0 | 0.0% | 89.3% |
| bash | 1802.42 | 24.032 | 0 | 0.0% | 100.0% |

## Performance Comparison

**Bashkit is 200.9x faster** than bash on average.

## Results by Category

### Arithmetic

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arith_basic | bashkit | 0.041 | ±0.016 | - | ✓ |
| arith_basic | bash | 8.480 | ±0.349 | - | ✓ |
| arith_complex | bashkit | 0.053 | ±0.023 | - | ✓ |
| arith_complex | bash | 9.501 | ±2.103 | - | ✓ |
| arith_variables | bashkit | 0.045 | ±0.020 | - | ✓ |
| arith_variables | bash | 8.678 | ±0.656 | - | ✓ |
| arith_increment | bashkit | 0.060 | ±0.018 | - | ✓ |
| arith_increment | bash | 8.681 | ±0.543 | - | ✓ |
| arith_modulo | bashkit | 0.050 | ±0.018 | - | ✓ |
| arith_modulo | bash | 8.875 | ±0.815 | - | ✓ |
| arith_loop_sum | bashkit | 0.093 | ±0.027 | - | ✓ |
| arith_loop_sum | bash | 8.375 | ±0.461 | - | ✓ |

### Arrays

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arr_create | bashkit | 0.055 | ±0.022 | - | ✓ |
| arr_create | bash | 9.295 | ±0.388 | - | ✓ |
| arr_all | bashkit | 0.051 | ±0.027 | - | ✓ |
| arr_all | bash | 8.981 | ±0.426 | - | ✓ |
| arr_length | bashkit | 0.043 | ±0.014 | - | ✓ |
| arr_length | bash | 9.069 | ±0.626 | - | ✓ |
| arr_iterate | bashkit | 0.050 | ±0.019 | - | ✓ |
| arr_iterate | bash | 8.921 | ±0.344 | - | ✓ |
| arr_slice | bashkit | 0.079 | ±0.061 | - | ✗ |
| arr_slice | bash | 9.402 | ±0.559 | - | ✓ |
| arr_assign_index | bashkit | 0.067 | ±0.025 | - | ✓ |
| arr_assign_index | bash | 9.123 | ±0.864 | - | ✓ |

### Complex

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| complex_fibonacci | bashkit | 1.436 | ±0.117 | - | ✓ |
| complex_fibonacci | bash | 698.912 | ±34.632 | - | ✓ |
| complex_fibonacci_iter | bashkit | 0.097 | ±0.026 | - | ✓ |
| complex_fibonacci_iter | bash | 8.796 | ±0.779 | - | ✓ |
| complex_nested_subst | bashkit | 0.050 | ±0.016 | - | ✓ |
| complex_nested_subst | bash | 22.169 | ±4.677 | - | ✓ |
| complex_loop_compute | bashkit | 0.076 | ±0.011 | - | ✓ |
| complex_loop_compute | bash | 10.026 | ±0.514 | - | ✓ |
| complex_string_build | bashkit | 0.044 | ±0.012 | - | ✓ |
| complex_string_build | bash | 9.561 | ±1.259 | - | ✓ |
| complex_json_transform | bashkit | 0.474 | ±0.020 | - | ✓ |
| complex_json_transform | bash | 25.916 | ±1.490 | - | ✓ |
| complex_pipeline_text | bashkit | 0.209 | ±0.048 | - | ✓ |
| complex_pipeline_text | bash | 26.904 | ±0.851 | - | ✓ |

### Control

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| ctrl_if_simple | bashkit | 0.044 | ±0.012 | - | ✓ |
| ctrl_if_simple | bash | 8.509 | ±0.220 | - | ✓ |
| ctrl_if_else | bashkit | 0.050 | ±0.017 | - | ✓ |
| ctrl_if_else | bash | 8.493 | ±0.570 | - | ✓ |
| ctrl_for_list | bashkit | 0.069 | ±0.027 | - | ✓ |
| ctrl_for_list | bash | 9.070 | ±0.343 | - | ✓ |
| ctrl_for_range | bashkit | 0.066 | ±0.021 | - | ✓ |
| ctrl_for_range | bash | 10.076 | ±2.022 | - | ✓ |
| ctrl_while | bashkit | 0.070 | ±0.021 | - | ✓ |
| ctrl_while | bash | 10.221 | ±2.465 | - | ✓ |
| ctrl_case | bashkit | 0.047 | ±0.015 | - | ✓ |
| ctrl_case | bash | 9.822 | ±2.075 | - | ✓ |
| ctrl_function | bashkit | 0.065 | ±0.038 | - | ✓ |
| ctrl_function | bash | 9.701 | ±1.264 | - | ✓ |
| ctrl_function_return | bashkit | 0.057 | ±0.014 | - | ✓ |
| ctrl_function_return | bash | 15.004 | ±2.967 | - | ✓ |
| ctrl_nested_loops | bashkit | 0.069 | ±0.043 | - | ✓ |
| ctrl_nested_loops | bash | 10.306 | ±1.034 | - | ✓ |

### Pipes

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| pipe_simple | bashkit | 0.041 | ±0.020 | - | ✓ |
| pipe_simple | bash | 21.066 | ±0.566 | - | ✓ |
| pipe_multi | bashkit | 0.046 | ±0.022 | - | ✓ |
| pipe_multi | bash | 27.899 | ±1.346 | - | ✓ |
| pipe_command_subst | bashkit | 0.049 | ±0.018 | - | ✓ |
| pipe_command_subst | bash | 13.019 | ±0.571 | - | ✓ |
| pipe_heredoc | bashkit | 0.060 | ±0.023 | - | ✓ |
| pipe_heredoc | bash | 19.487 | ±1.941 | - | ✓ |
| pipe_herestring | bashkit | 0.056 | ±0.033 | - | ✓ |
| pipe_herestring | bash | 19.264 | ±1.116 | - | ✓ |
| pipe_discard | bashkit | 0.034 | ±0.008 | - | ✓ |
| pipe_discard | bash | 13.401 | ±2.168 | - | ✓ |

### Startup

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| startup_empty | bashkit | 0.036 | ±0.013 | - | ✓ |
| startup_empty | bash | 10.217 | ±2.907 | - | ✓ |
| startup_true | bashkit | 0.039 | ±0.010 | - | ✓ |
| startup_true | bash | 8.449 | ±0.328 | - | ✓ |
| startup_echo | bashkit | 0.043 | ±0.018 | - | ✓ |
| startup_echo | bash | 8.498 | ±0.418 | - | ✓ |
| startup_exit | bashkit | 0.039 | ±0.013 | - | ✓ |
| startup_exit | bash | 8.399 | ±0.604 | - | ✓ |

### Strings

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| str_concat | bashkit | 0.060 | ±0.027 | - | ✓ |
| str_concat | bash | 9.261 | ±0.585 | - | ✓ |
| str_printf | bashkit | 0.039 | ±0.019 | - | ✓ |
| str_printf | bash | 9.194 | ±0.395 | - | ✓ |
| str_printf_pad | bashkit | 0.049 | ±0.032 | - | ✗ |
| str_printf_pad | bash | 9.241 | ±0.580 | - | ✓ |
| str_echo_escape | bashkit | 0.041 | ±0.022 | - | ✓ |
| str_echo_escape | bash | 9.136 | ±0.510 | - | ✓ |
| str_prefix_strip | bashkit | 0.064 | ±0.033 | - | ✓ |
| str_prefix_strip | bash | 9.163 | ±0.687 | - | ✓ |
| str_suffix_strip | bashkit | 0.045 | ±0.010 | - | ✓ |
| str_suffix_strip | bash | 9.053 | ±0.271 | - | ✓ |
| str_uppercase | bashkit | 0.039 | ±0.013 | - | ✗ |
| str_uppercase | bash | 9.321 | ±0.694 | - | ✓ |
| str_lowercase | bashkit | 0.049 | ±0.029 | - | ✗ |
| str_lowercase | bash | 9.775 | ±1.814 | - | ✓ |

### Tools

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| tool_grep_simple | bashkit | 0.068 | ±0.028 | - | ✓ |
| tool_grep_simple | bash | 22.555 | ±2.912 | - | ✓ |
| tool_grep_case | bashkit | 0.201 | ±0.104 | - | ✓ |
| tool_grep_case | bash | 21.500 | ±0.899 | - | ✓ |
| tool_grep_count | bashkit | 0.056 | ±0.032 | - | ✓ |
| tool_grep_count | bash | 21.550 | ±0.713 | - | ✓ |
| tool_grep_invert | bashkit | 0.067 | ±0.025 | - | ✓ |
| tool_grep_invert | bash | 22.238 | ±2.493 | - | ✓ |
| tool_grep_regex | bashkit | 0.094 | ±0.034 | - | ✓ |
| tool_grep_regex | bash | 22.569 | ±2.489 | - | ✓ |
| tool_sed_replace | bashkit | 0.166 | ±0.045 | - | ✓ |
| tool_sed_replace | bash | 24.719 | ±2.862 | - | ✓ |
| tool_sed_global | bashkit | 0.145 | ±0.021 | - | ✓ |
| tool_sed_global | bash | 23.764 | ±0.786 | - | ✓ |
| tool_sed_delete | bashkit | 0.060 | ±0.023 | - | ✓ |
| tool_sed_delete | bash | 24.521 | ±0.953 | - | ✓ |
| tool_sed_lines | bashkit | 0.046 | ±0.021 | - | ✓ |
| tool_sed_lines | bash | 24.496 | ±1.922 | - | ✓ |
| tool_sed_backrefs | bashkit | 0.163 | ±0.017 | - | ✗ |
| tool_sed_backrefs | bash | 27.609 | ±2.800 | - | ✓ |
| tool_awk_print | bashkit | 0.067 | ±0.034 | - | ✓ |
| tool_awk_print | bash | 22.336 | ±1.207 | - | ✓ |
| tool_awk_sum | bashkit | 0.062 | ±0.025 | - | ✓ |
| tool_awk_sum | bash | 23.620 | ±3.155 | - | ✓ |
| tool_awk_pattern | bashkit | 0.101 | ±0.031 | - | ✓ |
| tool_awk_pattern | bash | 23.028 | ±1.330 | - | ✓ |
| tool_awk_fieldsep | bashkit | 0.054 | ±0.029 | - | ✓ |
| tool_awk_fieldsep | bash | 22.593 | ±2.595 | - | ✓ |
| tool_awk_nf | bashkit | 0.055 | ±0.014 | - | ✓ |
| tool_awk_nf | bash | 22.281 | ±1.123 | - | ✓ |
| tool_awk_compute | bashkit | 0.078 | ±0.051 | - | ✓ |
| tool_awk_compute | bash | 22.912 | ±1.115 | - | ✓ |
| tool_jq_identity | bashkit | 0.484 | ±0.066 | - | ✓ |
| tool_jq_identity | bash | 28.634 | ±1.094 | - | ✓ |
| tool_jq_field | bashkit | 0.588 | ±0.068 | - | ✓ |
| tool_jq_field | bash | 26.513 | ±0.812 | - | ✓ |
| tool_jq_array | bashkit | 0.519 | ±0.084 | - | ✓ |
| tool_jq_array | bash | 26.946 | ±2.212 | - | ✓ |
| tool_jq_filter | bashkit | 0.508 | ±0.097 | - | ✓ |
| tool_jq_filter | bash | 26.823 | ±1.767 | - | ✓ |
| tool_jq_map | bashkit | 0.493 | ±0.048 | - | ✓ |
| tool_jq_map | bash | 26.650 | ±2.970 | - | ✓ |

### Variables

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| var_assign_simple | bashkit | 0.046 | ±0.022 | - | ✓ |
| var_assign_simple | bash | 8.373 | ±0.348 | - | ✓ |
| var_assign_many | bashkit | 0.089 | ±0.053 | - | ✓ |
| var_assign_many | bash | 8.868 | ±0.529 | - | ✓ |
| var_default | bashkit | 0.046 | ±0.011 | - | ✓ |
| var_default | bash | 8.773 | ±0.360 | - | ✓ |
| var_length | bashkit | 0.048 | ±0.018 | - | ✓ |
| var_length | bash | 9.028 | ±0.694 | - | ✓ |
| var_substring | bashkit | 0.053 | ±0.014 | - | ✗ |
| var_substring | bash | 8.442 | ±0.333 | - | ✓ |
| var_replace | bashkit | 0.058 | ±0.032 | - | ✗ |
| var_replace | bash | 8.557 | ±0.616 | - | ✓ |
| var_nested | bashkit | 0.057 | ±0.018 | - | ✗ |
| var_nested | bash | 9.418 | ±1.625 | - | ✓ |
| var_export | bashkit | 0.059 | ±0.019 | - | ✓ |
| var_export | bash | 8.393 | ±0.325 | - | ✓ |

## Assumptions & Notes

- Times measured in nanoseconds, displayed in milliseconds
- Prewarm phase runs first few cases to warm up JIT/compilation
- Per-benchmark warmup iterations excluded from timing
- Output match compares against bash output when available
- Errors include execution failures and exit code mismatches
- Bashkit runs in-process (no fork), bash spawns subprocess

