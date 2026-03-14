# Bashkit Benchmark Report

## System Information

- **Moniker**: `none-linux-x86_64`
- **Hostname**: (none)
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 4
- **Timestamp**: 1773457919
- **Iterations**: 10
- **Warmup**: 2
- **Prewarm cases**: 3

## Summary

Benchmarked 75 cases across 3 runners.

| Runner | Total Time (ms) | Avg/Case (ms) | Errors | Error Rate | Output Match |
|--------|-----------------|---------------|--------|------------|-------------|
| bashkit | 14.30 | 0.191 | 0 | 0.0% | 100.0% |
| bash | 319.35 | 4.258 | 0 | 0.0% | 100.0% |
| just-bash | 34095.47 | 454.606 | 0 | 0.0% | 100.0% |

## Performance Comparison

**Bashkit is 22.3x faster** than bash on average.

## Results by Category

### Arithmetic

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arith_basic | bashkit | 0.073 | ±0.017 | - | ✓ |
| arith_basic | bash | 1.661 | ±0.041 | - | ✓ |
| arith_basic | just-bash | 434.576 | ±3.783 | - | ✓ |
| arith_complex | bashkit | 0.079 | ±0.010 | - | ✓ |
| arith_complex | bash | 1.691 | ±0.051 | - | ✓ |
| arith_complex | just-bash | 435.617 | ±3.915 | - | ✓ |
| arith_variables | bashkit | 0.092 | ±0.017 | - | ✓ |
| arith_variables | bash | 1.737 | ±0.064 | - | ✓ |
| arith_variables | just-bash | 438.979 | ±5.846 | - | ✓ |
| arith_increment | bashkit | 0.073 | ±0.003 | - | ✓ |
| arith_increment | bash | 1.752 | ±0.156 | - | ✓ |
| arith_increment | just-bash | 446.120 | ±5.391 | - | ✓ |
| arith_modulo | bashkit | 0.072 | ±0.010 | - | ✓ |
| arith_modulo | bash | 1.773 | ±0.189 | - | ✓ |
| arith_modulo | just-bash | 438.544 | ±6.682 | - | ✓ |
| arith_loop_sum | bashkit | 0.124 | ±0.010 | - | ✓ |
| arith_loop_sum | bash | 1.731 | ±0.059 | - | ✓ |
| arith_loop_sum | just-bash | 462.889 | ±8.186 | - | ✓ |

### Arrays

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arr_create | bashkit | 0.154 | ±0.191 | - | ✓ |
| arr_create | bash | 1.722 | ±0.048 | - | ✓ |
| arr_create | just-bash | 453.131 | ±8.222 | - | ✓ |
| arr_all | bashkit | 0.085 | ±0.005 | - | ✓ |
| arr_all | bash | 1.834 | ±0.143 | - | ✓ |
| arr_all | just-bash | 456.795 | ±10.767 | - | ✓ |
| arr_length | bashkit | 0.089 | ±0.013 | - | ✓ |
| arr_length | bash | 1.730 | ±0.057 | - | ✓ |
| arr_length | just-bash | 450.888 | ±5.207 | - | ✓ |
| arr_iterate | bashkit | 0.100 | ±0.014 | - | ✓ |
| arr_iterate | bash | 1.776 | ±0.053 | - | ✓ |
| arr_iterate | just-bash | 458.060 | ±16.447 | - | ✓ |
| arr_slice | bashkit | 0.083 | ±0.006 | - | ✓ |
| arr_slice | bash | 1.901 | ±0.210 | - | ✓ |
| arr_slice | just-bash | 453.347 | ±4.782 | - | ✓ |
| arr_assign_index | bashkit | 0.082 | ±0.015 | - | ✓ |
| arr_assign_index | bash | 1.753 | ±0.077 | - | ✓ |
| arr_assign_index | just-bash | 455.700 | ±8.146 | - | ✓ |

### Complex

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| complex_fibonacci | bashkit | 2.997 | ±0.048 | - | ✓ |
| complex_fibonacci | bash | 125.499 | ±2.299 | - | ✓ |
| complex_fibonacci | just-bash | 520.309 | ±9.423 | - | ✓ |
| complex_fibonacci_iter | bashkit | 0.172 | ±0.016 | - | ✓ |
| complex_fibonacci_iter | bash | 1.811 | ±0.129 | - | ✓ |
| complex_fibonacci_iter | just-bash | 462.015 | ±9.375 | - | ✓ |
| complex_nested_subst | bashkit | 0.092 | ±0.009 | - | ✓ |
| complex_nested_subst | bash | 3.578 | ±0.242 | - | ✓ |
| complex_nested_subst | just-bash | 440.797 | ±6.900 | - | ✓ |
| complex_loop_compute | bashkit | 0.150 | ±0.012 | - | ✓ |
| complex_loop_compute | bash | 1.855 | ±0.224 | - | ✓ |
| complex_loop_compute | just-bash | 462.302 | ±8.441 | - | ✓ |
| complex_string_build | bashkit | 0.088 | ±0.007 | - | ✓ |
| complex_string_build | bash | 1.839 | ±0.167 | - | ✓ |
| complex_string_build | just-bash | 452.258 | ±13.503 | - | ✓ |
| complex_json_transform | bashkit | 0.668 | ±0.018 | - | ✓ |
| complex_json_transform | bash | 4.880 | ±0.143 | - | ✓ |
| complex_json_transform | just-bash | 435.865 | ±4.926 | - | ✓ |
| complex_pipeline_text | bashkit | 0.233 | ±0.014 | - | ✓ |
| complex_pipeline_text | bash | 3.696 | ±0.179 | - | ✓ |
| complex_pipeline_text | just-bash | 445.272 | ±6.865 | - | ✓ |

### Control

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| ctrl_if_simple | bashkit | 0.071 | ±0.010 | - | ✓ |
| ctrl_if_simple | bash | 1.687 | ±0.063 | - | ✓ |
| ctrl_if_simple | just-bash | 441.541 | ±9.574 | - | ✓ |
| ctrl_if_else | bashkit | 0.074 | ±0.009 | - | ✓ |
| ctrl_if_else | bash | 1.733 | ±0.177 | - | ✓ |
| ctrl_if_else | just-bash | 437.412 | ±8.657 | - | ✓ |
| ctrl_for_list | bashkit | 0.140 | ±0.146 | - | ✓ |
| ctrl_for_list | bash | 1.762 | ±0.179 | - | ✓ |
| ctrl_for_list | just-bash | 455.630 | ±14.011 | - | ✓ |
| ctrl_for_range | bashkit | 0.099 | ±0.009 | - | ✓ |
| ctrl_for_range | bash | 1.729 | ±0.036 | - | ✓ |
| ctrl_for_range | just-bash | 440.685 | ±3.572 | - | ✓ |
| ctrl_while | bashkit | 0.118 | ±0.011 | - | ✓ |
| ctrl_while | bash | 1.811 | ±0.146 | - | ✓ |
| ctrl_while | just-bash | 459.725 | ±11.145 | - | ✓ |
| ctrl_case | bashkit | 0.081 | ±0.004 | - | ✓ |
| ctrl_case | bash | 1.822 | ±0.177 | - | ✓ |
| ctrl_case | just-bash | 463.145 | ±10.997 | - | ✓ |
| ctrl_function | bashkit | 0.087 | ±0.003 | - | ✓ |
| ctrl_function | bash | 1.849 | ±0.168 | - | ✓ |
| ctrl_function | just-bash | 453.527 | ±6.432 | - | ✓ |
| ctrl_function_return | bashkit | 0.108 | ±0.007 | - | ✓ |
| ctrl_function_return | bash | 2.432 | ±0.150 | - | ✓ |
| ctrl_function_return | just-bash | 469.637 | ±19.143 | - | ✓ |
| ctrl_nested_loops | bashkit | 0.134 | ±0.016 | - | ✓ |
| ctrl_nested_loops | bash | 1.865 | ±0.293 | - | ✓ |
| ctrl_nested_loops | just-bash | 475.675 | ±9.892 | - | ✓ |

### Pipes

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| pipe_simple | bashkit | 0.074 | ±0.018 | - | ✓ |
| pipe_simple | bash | 3.456 | ±0.186 | - | ✓ |
| pipe_simple | just-bash | 452.697 | ±3.006 | - | ✓ |
| pipe_multi | bashkit | 0.069 | ±0.006 | - | ✓ |
| pipe_multi | bash | 3.689 | ±0.122 | - | ✓ |
| pipe_multi | just-bash | 451.817 | ±7.916 | - | ✓ |
| pipe_command_subst | bashkit | 0.074 | ±0.006 | - | ✓ |
| pipe_command_subst | bash | 2.378 | ±0.227 | - | ✓ |
| pipe_command_subst | just-bash | 452.759 | ±6.161 | - | ✓ |
| pipe_heredoc | bashkit | 0.067 | ±0.009 | - | ✓ |
| pipe_heredoc | bash | 3.376 | ±0.197 | - | ✓ |
| pipe_heredoc | just-bash | 445.271 | ±4.583 | - | ✓ |
| pipe_herestring | bashkit | 0.071 | ±0.006 | - | ✓ |
| pipe_herestring | bash | 3.308 | ±0.146 | - | ✓ |
| pipe_herestring | just-bash | 450.598 | ±11.701 | - | ✓ |
| pipe_discard | bashkit | 0.067 | ±0.011 | - | ✓ |
| pipe_discard | bash | 2.537 | ±0.202 | - | ✓ |
| pipe_discard | just-bash | 448.317 | ±3.387 | - | ✓ |

### Startup

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| startup_empty | bashkit | 0.060 | ±0.005 | - | ✓ |
| startup_empty | bash | 1.744 | ±0.065 | - | ✓ |
| startup_empty | just-bash | 434.700 | ±5.838 | - | ✓ |
| startup_true | bashkit | 0.074 | ±0.012 | - | ✓ |
| startup_true | bash | 1.847 | ±0.381 | - | ✓ |
| startup_true | just-bash | 440.313 | ±15.142 | - | ✓ |
| startup_echo | bashkit | 0.077 | ±0.009 | - | ✓ |
| startup_echo | bash | 1.674 | ±0.037 | - | ✓ |
| startup_echo | just-bash | 445.874 | ±8.038 | - | ✓ |
| startup_exit | bashkit | 0.123 | ±0.122 | - | ✓ |
| startup_exit | bash | 1.701 | ±0.169 | - | ✓ |
| startup_exit | just-bash | 440.150 | ±7.789 | - | ✓ |

### Strings

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| str_concat | bashkit | 0.071 | ±0.005 | - | ✓ |
| str_concat | bash | 1.815 | ±0.118 | - | ✓ |
| str_concat | just-bash | 477.770 | ±8.571 | - | ✓ |
| str_printf | bashkit | 0.070 | ±0.011 | - | ✓ |
| str_printf | bash | 1.756 | ±0.045 | - | ✓ |
| str_printf | just-bash | 471.250 | ±12.153 | - | ✓ |
| str_printf_pad | bashkit | 0.078 | ±0.006 | - | ✓ |
| str_printf_pad | bash | 1.786 | ±0.214 | - | ✓ |
| str_printf_pad | just-bash | 462.532 | ±9.980 | - | ✓ |
| str_echo_escape | bashkit | 0.085 | ±0.049 | - | ✓ |
| str_echo_escape | bash | 1.816 | ±0.063 | - | ✓ |
| str_echo_escape | just-bash | 460.618 | ±10.068 | - | ✓ |
| str_prefix_strip | bashkit | 0.114 | ±0.060 | - | ✓ |
| str_prefix_strip | bash | 1.761 | ±0.164 | - | ✓ |
| str_prefix_strip | just-bash | 463.257 | ±4.259 | - | ✓ |
| str_suffix_strip | bashkit | 0.072 | ±0.007 | - | ✓ |
| str_suffix_strip | bash | 1.798 | ±0.141 | - | ✓ |
| str_suffix_strip | just-bash | 457.155 | ±8.275 | - | ✓ |
| str_uppercase | bashkit | 0.083 | ±0.009 | - | ✓ |
| str_uppercase | bash | 1.742 | ±0.108 | - | ✓ |
| str_uppercase | just-bash | 450.078 | ±5.341 | - | ✓ |
| str_lowercase | bashkit | 0.106 | ±0.015 | - | ✓ |
| str_lowercase | bash | 1.748 | ±0.041 | - | ✓ |
| str_lowercase | just-bash | 449.912 | ±3.608 | - | ✓ |

### Tools

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| tool_grep_simple | bashkit | 0.090 | ±0.014 | - | ✓ |
| tool_grep_simple | bash | 3.527 | ±0.076 | - | ✓ |
| tool_grep_simple | just-bash | 460.854 | ±12.029 | - | ✓ |
| tool_grep_case | bashkit | 0.195 | ±0.021 | - | ✓ |
| tool_grep_case | bash | 3.681 | ±0.147 | - | ✓ |
| tool_grep_case | just-bash | 463.297 | ±8.148 | - | ✓ |
| tool_grep_count | bashkit | 0.078 | ±0.007 | - | ✓ |
| tool_grep_count | bash | 3.518 | ±0.148 | - | ✓ |
| tool_grep_count | just-bash | 450.963 | ±4.091 | - | ✓ |
| tool_grep_invert | bashkit | 0.101 | ±0.014 | - | ✓ |
| tool_grep_invert | bash | 3.597 | ±0.125 | - | ✓ |
| tool_grep_invert | just-bash | 454.332 | ±5.066 | - | ✓ |
| tool_grep_regex | bashkit | 0.154 | ±0.037 | - | ✓ |
| tool_grep_regex | bash | 3.674 | ±0.166 | - | ✓ |
| tool_grep_regex | just-bash | 457.451 | ±4.311 | - | ✓ |
| tool_sed_replace | bashkit | 0.225 | ±0.023 | - | ✓ |
| tool_sed_replace | bash | 3.861 | ±0.216 | - | ✓ |
| tool_sed_replace | just-bash | 452.104 | ±6.326 | - | ✓ |
| tool_sed_global | bashkit | 0.259 | ±0.065 | - | ✓ |
| tool_sed_global | bash | 3.781 | ±0.127 | - | ✓ |
| tool_sed_global | just-bash | 451.519 | ±5.075 | - | ✓ |
| tool_sed_delete | bashkit | 0.092 | ±0.015 | - | ✓ |
| tool_sed_delete | bash | 3.785 | ±0.105 | - | ✓ |
| tool_sed_delete | just-bash | 460.090 | ±10.279 | - | ✓ |
| tool_sed_lines | bashkit | 0.091 | ±0.012 | - | ✓ |
| tool_sed_lines | bash | 3.713 | ±0.061 | - | ✓ |
| tool_sed_lines | just-bash | 448.155 | ±7.909 | - | ✓ |
| tool_sed_backrefs | bashkit | 0.297 | ±0.037 | - | ✓ |
| tool_sed_backrefs | bash | 3.812 | ±0.058 | - | ✓ |
| tool_sed_backrefs | just-bash | 464.663 | ±7.097 | - | ✓ |
| tool_awk_print | bashkit | 0.104 | ±0.027 | - | ✓ |
| tool_awk_print | bash | 3.633 | ±0.116 | - | ✓ |
| tool_awk_print | just-bash | 460.490 | ±5.749 | - | ✓ |
| tool_awk_sum | bashkit | 0.118 | ±0.033 | - | ✓ |
| tool_awk_sum | bash | 3.687 | ±0.272 | - | ✓ |
| tool_awk_sum | just-bash | 473.226 | ±13.236 | - | ✓ |
| tool_awk_pattern | bashkit | 0.134 | ±0.017 | - | ✓ |
| tool_awk_pattern | bash | 3.600 | ±0.126 | - | ✓ |
| tool_awk_pattern | just-bash | 471.115 | ±7.676 | - | ✓ |
| tool_awk_fieldsep | bashkit | 0.080 | ±0.015 | - | ✓ |
| tool_awk_fieldsep | bash | 3.650 | ±0.109 | - | ✓ |
| tool_awk_fieldsep | just-bash | 464.425 | ±5.166 | - | ✓ |
| tool_awk_nf | bashkit | 0.076 | ±0.006 | - | ✓ |
| tool_awk_nf | bash | 3.452 | ±0.132 | - | ✓ |
| tool_awk_nf | just-bash | 472.080 | ±7.402 | - | ✓ |
| tool_awk_compute | bashkit | 0.097 | ±0.033 | - | ✓ |
| tool_awk_compute | bash | 3.672 | ±0.164 | - | ✓ |
| tool_awk_compute | just-bash | 471.298 | ±8.379 | - | ✓ |
| tool_jq_identity | bashkit | 0.691 | ±0.045 | - | ✓ |
| tool_jq_identity | bash | 5.222 | ±0.171 | - | ✓ |
| tool_jq_identity | just-bash | 470.611 | ±10.790 | - | ✓ |
| tool_jq_field | bashkit | 0.810 | ±0.156 | - | ✓ |
| tool_jq_field | bash | 5.207 | ±0.168 | - | ✓ |
| tool_jq_field | just-bash | 466.836 | ±6.700 | - | ✓ |
| tool_jq_array | bashkit | 0.702 | ±0.051 | - | ✓ |
| tool_jq_array | bash | 5.350 | ±0.158 | - | ✓ |
| tool_jq_array | just-bash | 460.364 | ±3.090 | - | ✓ |
| tool_jq_filter | bashkit | 0.705 | ±0.033 | - | ✓ |
| tool_jq_filter | bash | 5.228 | ±0.175 | - | ✓ |
| tool_jq_filter | just-bash | 467.175 | ±6.847 | - | ✓ |
| tool_jq_map | bashkit | 0.685 | ±0.050 | - | ✓ |
| tool_jq_map | bash | 5.146 | ±0.078 | - | ✓ |
| tool_jq_map | just-bash | 466.053 | ±12.104 | - | ✓ |

### Variables

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| var_assign_simple | bashkit | 0.096 | ±0.017 | - | ✓ |
| var_assign_simple | bash | 1.754 | ±0.165 | - | ✓ |
| var_assign_simple | just-bash | 448.787 | ±14.169 | - | ✓ |
| var_assign_many | bashkit | 0.106 | ±0.011 | - | ✓ |
| var_assign_many | bash | 1.677 | ±0.051 | - | ✓ |
| var_assign_many | just-bash | 439.392 | ±5.543 | - | ✓ |
| var_default | bashkit | 0.071 | ±0.009 | - | ✓ |
| var_default | bash | 1.733 | ±0.110 | - | ✓ |
| var_default | just-bash | 441.015 | ±10.378 | - | ✓ |
| var_length | bashkit | 0.070 | ±0.007 | - | ✓ |
| var_length | bash | 1.716 | ±0.077 | - | ✓ |
| var_length | just-bash | 440.801 | ±4.452 | - | ✓ |
| var_substring | bashkit | 0.075 | ±0.007 | - | ✓ |
| var_substring | bash | 1.690 | ±0.067 | - | ✓ |
| var_substring | just-bash | 439.661 | ±5.195 | - | ✓ |
| var_replace | bashkit | 0.078 | ±0.014 | - | ✓ |
| var_replace | bash | 1.707 | ±0.062 | - | ✓ |
| var_replace | just-bash | 440.703 | ±6.289 | - | ✓ |
| var_nested | bashkit | 0.079 | ±0.023 | - | ✓ |
| var_nested | bash | 1.699 | ±0.072 | - | ✓ |
| var_nested | just-bash | 442.854 | ±9.123 | - | ✓ |
| var_export | bashkit | 0.090 | ±0.029 | - | ✓ |
| var_export | bash | 1.740 | ±0.188 | - | ✓ |
| var_export | just-bash | 439.647 | ±9.871 | - | ✓ |

## Assumptions & Notes

- Times measured in nanoseconds, displayed in milliseconds
- Prewarm phase runs first few cases to warm up JIT/compilation
- Per-benchmark warmup iterations excluded from timing
- Output match compares against bash output when available
- Errors include execution failures and exit code mismatches
- Bashkit runs in-process (no fork), bash spawns subprocess

