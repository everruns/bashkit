# Bashkit Benchmark Report

## System Information

- **Moniker**: `none-linux-x86_64`
- **Hostname**: (none)
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 4
- **Timestamp**: 1773464548
- **Iterations**: 10
- **Warmup**: 2
- **Prewarm cases**: 3

## Summary

Benchmarked 96 cases across 7 runners.

| Runner | Total Time (ms) | Avg/Case (ms) | Errors | Error Rate | Output Match |
|--------|-----------------|---------------|--------|------------|-------------|
| bashkit | 33.11 | 0.345 | 0 | 0.0% | 100.0% |
| bashkit-cli | 785.83 | 8.186 | 0 | 0.0% | 100.0% |
| bashkit-js | 61.97 | 0.646 | 0 | 0.0% | 100.0% |
| bashkit-py | 49.28 | 0.513 | 0 | 0.0% | 100.0% |
| bash | 787.61 | 8.204 | 0 | 0.0% | 100.0% |
| just-bash | 35283.69 | 367.538 | 0 | 0.0% | 100.0% |
| just-bash-inproc | 428.01 | 4.458 | 0 | 0.0% | 100.0% |

## Performance Comparison

**Bashkit is 23.8x faster** than bash on average.

## Results by Category

### Arithmetic

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arith_basic | bashkit | 0.053 | ±0.002 | - | ✓ |
| arith_basic | bashkit-cli | 8.036 | ±0.309 | - | ✓ |
| arith_basic | bashkit-js | 0.311 | ±0.092 | - | ✓ |
| arith_basic | bashkit-py | 0.255 | ±0.063 | - | ✓ |
| arith_basic | bash | 1.518 | ±0.223 | - | ✓ |
| arith_basic | just-bash | 363.222 | ±9.578 | - | ✓ |
| arith_basic | just-bash-inproc | 1.533 | ±0.242 | - | ✓ |
| arith_complex | bashkit | 0.061 | ±0.009 | - | ✓ |
| arith_complex | bashkit-cli | 7.823 | ±0.442 | - | ✓ |
| arith_complex | bashkit-js | 0.235 | ±0.028 | - | ✓ |
| arith_complex | bashkit-py | 0.191 | ±0.019 | - | ✓ |
| arith_complex | bash | 1.333 | ±0.047 | - | ✓ |
| arith_complex | just-bash | 360.374 | ±5.469 | - | ✓ |
| arith_complex | just-bash-inproc | 1.597 | ±0.294 | - | ✓ |
| arith_variables | bashkit | 0.066 | ±0.007 | - | ✓ |
| arith_variables | bashkit-cli | 8.003 | ±0.341 | - | ✓ |
| arith_variables | bashkit-js | 0.265 | ±0.047 | - | ✓ |
| arith_variables | bashkit-py | 0.258 | ±0.026 | - | ✓ |
| arith_variables | bash | 1.449 | ±0.102 | - | ✓ |
| arith_variables | just-bash | 357.385 | ±2.859 | - | ✓ |
| arith_variables | just-bash-inproc | 1.507 | ±0.168 | - | ✓ |
| arith_increment | bashkit | 0.064 | ±0.010 | - | ✓ |
| arith_increment | bashkit-cli | 7.728 | ±0.163 | - | ✓ |
| arith_increment | bashkit-js | 0.278 | ±0.065 | - | ✓ |
| arith_increment | bashkit-py | 0.234 | ±0.033 | - | ✓ |
| arith_increment | bash | 1.476 | ±0.129 | - | ✓ |
| arith_increment | just-bash | 363.971 | ±6.949 | - | ✓ |
| arith_increment | just-bash-inproc | 1.751 | ±0.182 | - | ✓ |
| arith_modulo | bashkit | 0.056 | ±0.010 | - | ✓ |
| arith_modulo | bashkit-cli | 7.885 | ±0.386 | - | ✓ |
| arith_modulo | bashkit-js | 0.298 | ±0.055 | - | ✓ |
| arith_modulo | bashkit-py | 0.177 | ±0.013 | - | ✓ |
| arith_modulo | bash | 1.403 | ±0.157 | - | ✓ |
| arith_modulo | just-bash | 359.814 | ±6.413 | - | ✓ |
| arith_modulo | just-bash-inproc | 1.557 | ±0.239 | - | ✓ |
| arith_loop_sum | bashkit | 0.148 | ±0.055 | - | ✓ |
| arith_loop_sum | bashkit-cli | 7.704 | ±0.086 | - | ✓ |
| arith_loop_sum | bashkit-js | 0.277 | ±0.017 | - | ✓ |
| arith_loop_sum | bashkit-py | 0.326 | ±0.039 | - | ✓ |
| arith_loop_sum | bash | 1.434 | ±0.038 | - | ✓ |
| arith_loop_sum | just-bash | 364.608 | ±4.587 | - | ✓ |
| arith_loop_sum | just-bash-inproc | 2.308 | ±0.266 | - | ✓ |

### Arrays

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arr_create | bashkit | 0.063 | ±0.009 | - | ✓ |
| arr_create | bashkit-cli | 7.721 | ±0.155 | - | ✓ |
| arr_create | bashkit-js | 0.247 | ±0.036 | - | ✓ |
| arr_create | bashkit-py | 0.185 | ±0.016 | - | ✓ |
| arr_create | bash | 1.360 | ±0.036 | - | ✓ |
| arr_create | just-bash | 368.847 | ±24.173 | - | ✓ |
| arr_create | just-bash-inproc | 1.849 | ±0.232 | - | ✓ |
| arr_all | bashkit | 0.067 | ±0.017 | - | ✓ |
| arr_all | bashkit-cli | 7.904 | ±0.170 | - | ✓ |
| arr_all | bashkit-js | 0.249 | ±0.039 | - | ✓ |
| arr_all | bashkit-py | 0.240 | ±0.023 | - | ✓ |
| arr_all | bash | 1.421 | ±0.126 | - | ✓ |
| arr_all | just-bash | 358.798 | ±6.617 | - | ✓ |
| arr_all | just-bash-inproc | 1.666 | ±0.183 | - | ✓ |
| arr_length | bashkit | 0.062 | ±0.010 | - | ✓ |
| arr_length | bashkit-cli | 8.219 | ±0.566 | - | ✓ |
| arr_length | bashkit-js | 0.243 | ±0.035 | - | ✓ |
| arr_length | bashkit-py | 0.213 | ±0.025 | - | ✓ |
| arr_length | bash | 1.377 | ±0.084 | - | ✓ |
| arr_length | just-bash | 358.410 | ±4.141 | - | ✓ |
| arr_length | just-bash-inproc | 1.693 | ±0.174 | - | ✓ |
| arr_iterate | bashkit | 0.069 | ±0.012 | - | ✓ |
| arr_iterate | bashkit-cli | 8.048 | ±0.317 | - | ✓ |
| arr_iterate | bashkit-js | 0.234 | ±0.023 | - | ✓ |
| arr_iterate | bashkit-py | 0.230 | ±0.020 | - | ✓ |
| arr_iterate | bash | 1.492 | ±0.160 | - | ✓ |
| arr_iterate | just-bash | 374.739 | ±22.050 | - | ✓ |
| arr_iterate | just-bash-inproc | 1.802 | ±0.224 | - | ✓ |
| arr_slice | bashkit | 0.071 | ±0.018 | - | ✓ |
| arr_slice | bashkit-cli | 7.627 | ±0.190 | - | ✓ |
| arr_slice | bashkit-js | 0.254 | ±0.062 | - | ✓ |
| arr_slice | bashkit-py | 0.253 | ±0.020 | - | ✓ |
| arr_slice | bash | 1.436 | ±0.131 | - | ✓ |
| arr_slice | just-bash | 373.243 | ±25.921 | - | ✓ |
| arr_slice | just-bash-inproc | 1.873 | ±0.270 | - | ✓ |
| arr_assign_index | bashkit | 0.065 | ±0.006 | - | ✓ |
| arr_assign_index | bashkit-cli | 7.820 | ±0.457 | - | ✓ |
| arr_assign_index | bashkit-js | 0.210 | ±0.032 | - | ✓ |
| arr_assign_index | bashkit-py | 0.189 | ±0.018 | - | ✓ |
| arr_assign_index | bash | 1.396 | ±0.105 | - | ✓ |
| arr_assign_index | just-bash | 359.828 | ±3.750 | - | ✓ |
| arr_assign_index | just-bash-inproc | 2.115 | ±0.953 | - | ✓ |

### Complex

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| complex_fibonacci | bashkit | 2.568 | ±0.204 | - | ✓ |
| complex_fibonacci | bashkit-cli | 10.872 | ±0.445 | - | ✓ |
| complex_fibonacci | bashkit-js | 3.247 | ±0.494 | - | ✓ |
| complex_fibonacci | bashkit-py | 2.987 | ±0.238 | - | ✓ |
| complex_fibonacci | bash | 101.742 | ±2.276 | - | ✓ |
| complex_fibonacci | just-bash | 412.630 | ±5.408 | - | ✓ |
| complex_fibonacci | just-bash-inproc | 49.577 | ±9.617 | - | ✓ |
| complex_fibonacci_iter | bashkit | 0.142 | ±0.021 | - | ✓ |
| complex_fibonacci_iter | bashkit-cli | 7.897 | ±0.276 | - | ✓ |
| complex_fibonacci_iter | bashkit-js | 0.351 | ±0.032 | - | ✓ |
| complex_fibonacci_iter | bashkit-py | 0.329 | ±0.071 | - | ✓ |
| complex_fibonacci_iter | bash | 1.559 | ±0.075 | - | ✓ |
| complex_fibonacci_iter | just-bash | 367.044 | ±3.725 | - | ✓ |
| complex_fibonacci_iter | just-bash-inproc | 2.310 | ±0.368 | - | ✓ |
| complex_nested_subst | bashkit | 0.075 | ±0.010 | - | ✓ |
| complex_nested_subst | bashkit-cli | 7.847 | ±0.247 | - | ✓ |
| complex_nested_subst | bashkit-js | 0.230 | ±0.041 | - | ✓ |
| complex_nested_subst | bashkit-py | 0.286 | ±0.080 | - | ✓ |
| complex_nested_subst | bash | 3.069 | ±0.286 | - | ✓ |
| complex_nested_subst | just-bash | 357.805 | ±5.354 | - | ✓ |
| complex_nested_subst | just-bash-inproc | 1.537 | ±0.288 | - | ✓ |
| complex_loop_compute | bashkit | 0.144 | ±0.022 | - | ✓ |
| complex_loop_compute | bashkit-cli | 7.910 | ±0.176 | - | ✓ |
| complex_loop_compute | bashkit-js | 0.376 | ±0.093 | - | ✓ |
| complex_loop_compute | bashkit-py | 0.338 | ±0.061 | - | ✓ |
| complex_loop_compute | bash | 1.448 | ±0.055 | - | ✓ |
| complex_loop_compute | just-bash | 369.944 | ±10.190 | - | ✓ |
| complex_loop_compute | just-bash-inproc | 2.210 | ±0.214 | - | ✓ |
| complex_string_build | bashkit | 0.125 | ±0.033 | - | ✓ |
| complex_string_build | bashkit-cli | 8.030 | ±0.634 | - | ✓ |
| complex_string_build | bashkit-js | 0.278 | ±0.081 | - | ✓ |
| complex_string_build | bashkit-py | 0.304 | ±0.115 | - | ✓ |
| complex_string_build | bash | 1.475 | ±0.208 | - | ✓ |
| complex_string_build | just-bash | 371.412 | ±17.587 | - | ✓ |
| complex_string_build | just-bash-inproc | 1.635 | ±0.097 | - | ✓ |
| complex_json_transform | bashkit | 0.587 | ±0.025 | - | ✓ |
| complex_json_transform | bashkit-cli | 9.751 | ±1.462 | - | ✓ |
| complex_json_transform | bashkit-js | 0.918 | ±0.109 | - | ✓ |
| complex_json_transform | bashkit-py | 0.903 | ±0.035 | - | ✓ |
| complex_json_transform | bash | 4.350 | ±0.149 | - | ✓ |
| complex_json_transform | just-bash | 364.422 | ±4.818 | - | ✓ |
| complex_json_transform | just-bash-inproc | 1.536 | ±0.215 | - | ✓ |
| complex_pipeline_text | bashkit | 0.215 | ±0.033 | - | ✓ |
| complex_pipeline_text | bashkit-cli | 8.467 | ±0.320 | - | ✓ |
| complex_pipeline_text | bashkit-js | 0.474 | ±0.056 | - | ✓ |
| complex_pipeline_text | bashkit-py | 0.490 | ±0.069 | - | ✓ |
| complex_pipeline_text | bash | 3.218 | ±0.084 | - | ✓ |
| complex_pipeline_text | just-bash | 377.465 | ±9.973 | - | ✓ |
| complex_pipeline_text | just-bash-inproc | 1.947 | ±0.224 | - | ✓ |

### Control

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| ctrl_if_simple | bashkit | 0.063 | ±0.008 | - | ✓ |
| ctrl_if_simple | bashkit-cli | 7.774 | ±0.225 | - | ✓ |
| ctrl_if_simple | bashkit-js | 0.247 | ±0.036 | - | ✓ |
| ctrl_if_simple | bashkit-py | 0.178 | ±0.015 | - | ✓ |
| ctrl_if_simple | bash | 1.422 | ±0.067 | - | ✓ |
| ctrl_if_simple | just-bash | 358.368 | ±12.582 | - | ✓ |
| ctrl_if_simple | just-bash-inproc | 1.543 | ±0.165 | - | ✓ |
| ctrl_if_else | bashkit | 0.066 | ±0.014 | - | ✓ |
| ctrl_if_else | bashkit-cli | 7.856 | ±0.165 | - | ✓ |
| ctrl_if_else | bashkit-js | 0.199 | ±0.039 | - | ✓ |
| ctrl_if_else | bashkit-py | 0.220 | ±0.023 | - | ✓ |
| ctrl_if_else | bash | 1.324 | ±0.046 | - | ✓ |
| ctrl_if_else | just-bash | 354.779 | ±4.606 | - | ✓ |
| ctrl_if_else | just-bash-inproc | 1.531 | ±0.220 | - | ✓ |
| ctrl_for_list | bashkit | 0.080 | ±0.009 | - | ✓ |
| ctrl_for_list | bashkit-cli | 7.760 | ±0.330 | - | ✓ |
| ctrl_for_list | bashkit-js | 0.301 | ±0.050 | - | ✓ |
| ctrl_for_list | bashkit-py | 0.342 | ±0.081 | - | ✓ |
| ctrl_for_list | bash | 1.452 | ±0.086 | - | ✓ |
| ctrl_for_list | just-bash | 359.751 | ±6.237 | - | ✓ |
| ctrl_for_list | just-bash-inproc | 2.354 | ±0.344 | - | ✓ |
| ctrl_for_range | bashkit | 0.086 | ±0.017 | - | ✓ |
| ctrl_for_range | bashkit-cli | 7.823 | ±0.278 | - | ✓ |
| ctrl_for_range | bashkit-js | 0.347 | ±0.047 | - | ✓ |
| ctrl_for_range | bashkit-py | 0.314 | ±0.058 | - | ✓ |
| ctrl_for_range | bash | 1.481 | ±0.124 | - | ✓ |
| ctrl_for_range | just-bash | 368.502 | ±7.572 | - | ✓ |
| ctrl_for_range | just-bash-inproc | 2.133 | ±0.118 | - | ✓ |
| ctrl_while | bashkit | 0.112 | ±0.020 | - | ✓ |
| ctrl_while | bashkit-cli | 8.088 | ±0.345 | - | ✓ |
| ctrl_while | bashkit-js | 0.325 | ±0.061 | - | ✓ |
| ctrl_while | bashkit-py | 0.310 | ±0.045 | - | ✓ |
| ctrl_while | bash | 1.549 | ±0.081 | - | ✓ |
| ctrl_while | just-bash | 375.022 | ±9.714 | - | ✓ |
| ctrl_while | just-bash-inproc | 3.155 | ±0.331 | - | ✓ |
| ctrl_case | bashkit | 0.074 | ±0.006 | - | ✓ |
| ctrl_case | bashkit-cli | 7.906 | ±0.161 | - | ✓ |
| ctrl_case | bashkit-js | 0.322 | ±0.062 | - | ✓ |
| ctrl_case | bashkit-py | 0.235 | ±0.040 | - | ✓ |
| ctrl_case | bash | 1.483 | ±0.063 | - | ✓ |
| ctrl_case | just-bash | 359.762 | ±2.701 | - | ✓ |
| ctrl_case | just-bash-inproc | 1.985 | ±0.246 | - | ✓ |
| ctrl_function | bashkit | 0.059 | ±0.003 | - | ✓ |
| ctrl_function | bashkit-cli | 7.993 | ±0.550 | - | ✓ |
| ctrl_function | bashkit-js | 0.239 | ±0.047 | - | ✓ |
| ctrl_function | bashkit-py | 0.307 | ±0.063 | - | ✓ |
| ctrl_function | bash | 1.400 | ±0.048 | - | ✓ |
| ctrl_function | just-bash | 364.491 | ±24.128 | - | ✓ |
| ctrl_function | just-bash-inproc | 1.811 | ±0.262 | - | ✓ |
| ctrl_function_return | bashkit | 0.076 | ±0.009 | - | ✓ |
| ctrl_function_return | bashkit-cli | 7.782 | ±0.204 | - | ✓ |
| ctrl_function_return | bashkit-js | 0.348 | ±0.129 | - | ✓ |
| ctrl_function_return | bashkit-py | 0.377 | ±0.054 | - | ✓ |
| ctrl_function_return | bash | 2.203 | ±0.442 | - | ✓ |
| ctrl_function_return | just-bash | 370.644 | ±11.804 | - | ✓ |
| ctrl_function_return | just-bash-inproc | 2.018 | ±0.227 | - | ✓ |
| ctrl_nested_loops | bashkit | 0.099 | ±0.011 | - | ✓ |
| ctrl_nested_loops | bashkit-cli | 8.184 | ±0.376 | - | ✓ |
| ctrl_nested_loops | bashkit-js | 0.332 | ±0.065 | - | ✓ |
| ctrl_nested_loops | bashkit-py | 0.381 | ±0.119 | - | ✓ |
| ctrl_nested_loops | bash | 1.744 | ±0.511 | - | ✓ |
| ctrl_nested_loops | just-bash | 369.431 | ±8.602 | - | ✓ |
| ctrl_nested_loops | just-bash-inproc | 2.603 | ±0.230 | - | ✓ |

### Io

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| io_redirect_write | bashkit | 0.070 | ±0.014 | - | ✓ |
| io_redirect_write | bashkit-cli | 7.747 | ±0.212 | - | ✓ |
| io_redirect_write | bashkit-js | 0.231 | ±0.028 | - | ✓ |
| io_redirect_write | bashkit-py | 0.219 | ±0.034 | - | ✓ |
| io_redirect_write | bash | 3.726 | ±0.170 | - | ✓ |
| io_redirect_write | just-bash | 356.185 | ±3.131 | - | ✓ |
| io_redirect_write | just-bash-inproc | 1.846 | ±0.134 | - | ✓ |
| io_append | bashkit | 0.094 | ±0.028 | - | ✓ |
| io_append | bashkit-cli | 8.008 | ±0.427 | - | ✓ |
| io_append | bashkit-js | 0.448 | ±0.359 | - | ✓ |
| io_append | bashkit-py | 0.217 | ±0.035 | - | ✓ |
| io_append | bash | 3.679 | ±0.160 | - | ✓ |
| io_append | just-bash | 364.834 | ±11.561 | - | ✓ |
| io_append | just-bash-inproc | 1.769 | ±0.121 | - | ✓ |
| io_dev_null | bashkit | 0.055 | ±0.005 | - | ✓ |
| io_dev_null | bashkit-cli | 7.584 | ±0.229 | - | ✓ |
| io_dev_null | bashkit-js | 0.214 | ±0.041 | - | ✓ |
| io_dev_null | bashkit-py | 0.190 | ±0.034 | - | ✓ |
| io_dev_null | bash | 1.349 | ±0.059 | - | ✓ |
| io_dev_null | just-bash | 357.893 | ±5.819 | - | ✓ |
| io_dev_null | just-bash-inproc | 1.450 | ±0.183 | - | ✓ |
| io_stderr_redirect | bashkit | 0.055 | ±0.007 | - | ✓ |
| io_stderr_redirect | bashkit-cli | 7.671 | ±0.464 | - | ✓ |
| io_stderr_redirect | bashkit-js | 0.195 | ±0.036 | - | ✓ |
| io_stderr_redirect | bashkit-py | 0.222 | ±0.035 | - | ✓ |
| io_stderr_redirect | bash | 1.343 | ±0.042 | - | ✓ |
| io_stderr_redirect | just-bash | 357.659 | ±4.649 | - | ✓ |
| io_stderr_redirect | just-bash-inproc | 1.465 | ±0.154 | - | ✓ |
| io_read_lines | bashkit | 0.117 | ±0.020 | - | ✓ |
| io_read_lines | bashkit-cli | 7.784 | ±0.174 | - | ✓ |
| io_read_lines | bashkit-js | 0.406 | ±0.053 | - | ✓ |
| io_read_lines | bashkit-py | 0.401 | ±0.034 | - | ✓ |
| io_read_lines | bash | 1.526 | ±0.118 | - | ✓ |
| io_read_lines | just-bash | 372.336 | ±5.588 | - | ✓ |
| io_read_lines | just-bash-inproc | 2.120 | ±0.388 | - | ✓ |
| io_multiline_heredoc | bashkit | 0.063 | ±0.009 | - | ✓ |
| io_multiline_heredoc | bashkit-cli | 7.954 | ±0.325 | - | ✓ |
| io_multiline_heredoc | bashkit-js | 0.277 | ±0.063 | - | ✓ |
| io_multiline_heredoc | bashkit-py | 0.240 | ±0.024 | - | ✓ |
| io_multiline_heredoc | bash | 2.818 | ±0.085 | - | ✓ |
| io_multiline_heredoc | just-bash | 363.966 | ±5.739 | - | ✓ |
| io_multiline_heredoc | just-bash-inproc | 1.475 | ±0.200 | - | ✓ |

### Large

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| large_loop_1000 | bashkit | 4.948 | ±0.173 | - | ✓ |
| large_loop_1000 | bashkit-cli | 11.874 | ±0.163 | - | ✓ |
| large_loop_1000 | bashkit-js | 4.405 | ±0.140 | - | ✓ |
| large_loop_1000 | bashkit-py | 4.268 | ±0.066 | - | ✓ |
| large_loop_1000 | bash | 4.218 | ±0.259 | - | ✓ |
| large_loop_1000 | just-bash | 398.799 | ±8.179 | - | ✓ |
| large_loop_1000 | just-bash-inproc | 20.998 | ±0.959 | - | ✓ |
| large_string_append_100 | bashkit | 0.433 | ±0.032 | - | ✓ |
| large_string_append_100 | bashkit-cli | 8.040 | ±0.149 | - | ✓ |
| large_string_append_100 | bashkit-js | 0.559 | ±0.063 | - | ✓ |
| large_string_append_100 | bashkit-py | 0.621 | ±0.108 | - | ✓ |
| large_string_append_100 | bash | 1.748 | ±0.127 | - | ✓ |
| large_string_append_100 | just-bash | 367.447 | ±4.463 | - | ✓ |
| large_string_append_100 | just-bash-inproc | 3.615 | ±0.214 | - | ✓ |
| large_array_fill_200 | bashkit | 0.729 | ±0.069 | - | ✓ |
| large_array_fill_200 | bashkit-cli | 8.565 | ±0.177 | - | ✓ |
| large_array_fill_200 | bashkit-js | 1.065 | ±0.086 | - | ✓ |
| large_array_fill_200 | bashkit-py | 0.989 | ±0.053 | - | ✓ |
| large_array_fill_200 | bash | 1.990 | ±0.084 | - | ✓ |
| large_array_fill_200 | just-bash | 385.916 | ±6.698 | - | ✓ |
| large_array_fill_200 | just-bash-inproc | 18.000 | ±1.173 | - | ✓ |
| large_nested_loops | bashkit | 2.178 | ±0.125 | - | ✓ |
| large_nested_loops | bashkit-cli | 9.894 | ±0.521 | - | ✓ |
| large_nested_loops | bashkit-js | 2.686 | ±0.160 | - | ✓ |
| large_nested_loops | bashkit-py | 2.193 | ±0.064 | - | ✓ |
| large_nested_loops | bash | 2.667 | ±0.125 | - | ✓ |
| large_nested_loops | just-bash | 385.456 | ±7.081 | - | ✓ |
| large_nested_loops | just-bash-inproc | 10.264 | ±0.263 | - | ✓ |
| large_fibonacci_12 | bashkit | 6.464 | ±0.163 | - | ✓ |
| large_fibonacci_12 | bashkit-cli | 15.573 | ±1.273 | - | ✓ |
| large_fibonacci_12 | bashkit-js | 6.980 | ±0.355 | - | ✓ |
| large_fibonacci_12 | bashkit-py | 6.837 | ±0.076 | - | ✓ |
| large_fibonacci_12 | bash | 265.518 | ±4.757 | - | ✓ |
| large_fibonacci_12 | just-bash | 468.578 | ±12.092 | - | ✓ |
| large_fibonacci_12 | just-bash-inproc | 98.340 | ±11.183 | - | ✓ |
| large_function_calls_500 | bashkit | 4.079 | ±0.118 | - | ✓ |
| large_function_calls_500 | bashkit-cli | 11.907 | ±0.677 | - | ✓ |
| large_function_calls_500 | bashkit-js | 4.563 | ±0.983 | - | ✓ |
| large_function_calls_500 | bashkit-py | 3.811 | ±0.129 | - | ✓ |
| large_function_calls_500 | bash | 214.953 | ±9.054 | - | ✓ |
| large_function_calls_500 | just-bash | 431.876 | ±5.175 | - | ✓ |
| large_function_calls_500 | just-bash-inproc | 52.877 | ±2.774 | - | ✓ |
| large_multiline_script | bashkit | 0.478 | ±0.025 | - | ✓ |
| large_multiline_script | bashkit-cli | 8.172 | ±0.259 | - | ✓ |
| large_multiline_script | bashkit-js | 0.724 | ±0.071 | - | ✓ |
| large_multiline_script | bashkit-py | 0.680 | ±0.044 | - | ✓ |
| large_multiline_script | bash | 1.735 | ±0.068 | - | ✓ |
| large_multiline_script | just-bash | 376.460 | ±3.915 | - | ✓ |
| large_multiline_script | just-bash-inproc | 4.223 | ±0.306 | - | ✓ |
| large_pipeline_chain | bashkit | 0.857 | ±0.075 | - | ✓ |
| large_pipeline_chain | bashkit-cli | 8.735 | ±0.190 | - | ✓ |
| large_pipeline_chain | bashkit-js | 1.051 | ±0.052 | - | ✓ |
| large_pipeline_chain | bashkit-py | 1.022 | ±0.133 | - | ✓ |
| large_pipeline_chain | bash | 3.170 | ±0.131 | - | ✓ |
| large_pipeline_chain | just-bash | 403.028 | ±5.252 | - | ✓ |
| large_pipeline_chain | just-bash-inproc | 15.264 | ±0.986 | - | ✓ |
| large_assoc_array | bashkit | 0.079 | ±0.015 | - | ✓ |
| large_assoc_array | bashkit-cli | 7.618 | ±0.109 | - | ✓ |
| large_assoc_array | bashkit-js | 0.255 | ±0.037 | - | ✓ |
| large_assoc_array | bashkit-py | 0.215 | ±0.041 | - | ✓ |
| large_assoc_array | bash | 1.350 | ±0.049 | - | ✓ |
| large_assoc_array | just-bash | 363.625 | ±7.672 | - | ✓ |
| large_assoc_array | just-bash-inproc | 1.498 | ±0.144 | - | ✓ |

### Pipes

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| pipe_simple | bashkit | 0.060 | ±0.012 | - | ✓ |
| pipe_simple | bashkit-cli | 7.748 | ±0.166 | - | ✓ |
| pipe_simple | bashkit-js | 0.248 | ±0.073 | - | ✓ |
| pipe_simple | bashkit-py | 0.198 | ±0.032 | - | ✓ |
| pipe_simple | bash | 2.786 | ±0.120 | - | ✓ |
| pipe_simple | just-bash | 362.695 | ±6.855 | - | ✓ |
| pipe_simple | just-bash-inproc | 1.608 | ±0.211 | - | ✓ |
| pipe_multi | bashkit | 0.079 | ±0.036 | - | ✓ |
| pipe_multi | bashkit-cli | 8.030 | ±0.384 | - | ✓ |
| pipe_multi | bashkit-js | 0.286 | ±0.067 | - | ✓ |
| pipe_multi | bashkit-py | 0.301 | ±0.024 | - | ✓ |
| pipe_multi | bash | 3.164 | ±0.318 | - | ✓ |
| pipe_multi | just-bash | 361.922 | ±5.171 | - | ✓ |
| pipe_multi | just-bash-inproc | 1.937 | ±0.243 | - | ✓ |
| pipe_command_subst | bashkit | 0.062 | ±0.005 | - | ✓ |
| pipe_command_subst | bashkit-cli | 7.914 | ±0.388 | - | ✓ |
| pipe_command_subst | bashkit-js | 0.250 | ±0.053 | - | ✓ |
| pipe_command_subst | bashkit-py | 0.270 | ±0.041 | - | ✓ |
| pipe_command_subst | bash | 1.807 | ±0.037 | - | ✓ |
| pipe_command_subst | just-bash | 356.073 | ±5.913 | - | ✓ |
| pipe_command_subst | just-bash-inproc | 1.567 | ±0.175 | - | ✓ |
| pipe_heredoc | bashkit | 0.053 | ±0.006 | - | ✓ |
| pipe_heredoc | bashkit-cli | 7.875 | ±0.559 | - | ✓ |
| pipe_heredoc | bashkit-js | 0.220 | ±0.049 | - | ✓ |
| pipe_heredoc | bashkit-py | 0.174 | ±0.015 | - | ✓ |
| pipe_heredoc | bash | 2.691 | ±0.104 | - | ✓ |
| pipe_heredoc | just-bash | 351.963 | ±3.588 | - | ✓ |
| pipe_heredoc | just-bash-inproc | 1.605 | ±0.239 | - | ✓ |
| pipe_herestring | bashkit | 0.052 | ±0.009 | - | ✓ |
| pipe_herestring | bashkit-cli | 7.728 | ±0.144 | - | ✓ |
| pipe_herestring | bashkit-js | 0.347 | ±0.070 | - | ✓ |
| pipe_herestring | bashkit-py | 0.211 | ±0.046 | - | ✓ |
| pipe_herestring | bash | 2.692 | ±0.114 | - | ✓ |
| pipe_herestring | just-bash | 351.470 | ±3.880 | - | ✓ |
| pipe_herestring | just-bash-inproc | 1.383 | ±0.236 | - | ✓ |
| pipe_discard | bashkit | 0.059 | ±0.004 | - | ✓ |
| pipe_discard | bashkit-cli | 7.971 | ±0.492 | - | ✓ |
| pipe_discard | bashkit-js | 0.223 | ±0.047 | - | ✓ |
| pipe_discard | bashkit-py | 0.224 | ±0.086 | - | ✓ |
| pipe_discard | bash | 1.908 | ±0.091 | - | ✓ |
| pipe_discard | just-bash | 352.479 | ±4.044 | - | ✓ |
| pipe_discard | just-bash-inproc | 1.566 | ±0.191 | - | ✓ |

### Startup

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| startup_empty | bashkit | 0.052 | ±0.007 | - | ✓ |
| startup_empty | bashkit-cli | 7.820 | ±0.377 | - | ✓ |
| startup_empty | bashkit-js | 0.338 | ±0.118 | - | ✓ |
| startup_empty | bashkit-py | 0.296 | ±0.165 | - | ✓ |
| startup_empty | bash | 1.764 | ±0.559 | - | ✓ |
| startup_empty | just-bash | 355.389 | ±8.682 | - | ✓ |
| startup_empty | just-bash-inproc | 2.135 | ±1.202 | - | ✓ |
| startup_true | bashkit | 0.052 | ±0.003 | - | ✓ |
| startup_true | bashkit-cli | 7.648 | ±0.150 | - | ✓ |
| startup_true | bashkit-js | 1.566 | ±3.928 | - | ✓ |
| startup_true | bashkit-py | 0.214 | ±0.032 | - | ✓ |
| startup_true | bash | 1.456 | ±0.126 | - | ✓ |
| startup_true | just-bash | 357.317 | ±14.727 | - | ✓ |
| startup_true | just-bash-inproc | 1.402 | ±0.120 | - | ✓ |
| startup_echo | bashkit | 0.056 | ±0.007 | - | ✓ |
| startup_echo | bashkit-cli | 7.693 | ±0.112 | - | ✓ |
| startup_echo | bashkit-js | 0.225 | ±0.035 | - | ✓ |
| startup_echo | bashkit-py | 0.180 | ±0.016 | - | ✓ |
| startup_echo | bash | 1.370 | ±0.065 | - | ✓ |
| startup_echo | just-bash | 358.161 | ±5.631 | - | ✓ |
| startup_echo | just-bash-inproc | 1.789 | ±0.301 | - | ✓ |
| startup_exit | bashkit | 0.071 | ±0.011 | - | ✓ |
| startup_exit | bashkit-cli | 7.806 | ±0.242 | - | ✓ |
| startup_exit | bashkit-js | 1.933 | ±5.031 | - | ✓ |
| startup_exit | bashkit-py | 0.206 | ±0.026 | - | ✓ |
| startup_exit | bash | 1.416 | ±0.100 | - | ✓ |
| startup_exit | just-bash | 354.144 | ±8.186 | - | ✓ |
| startup_exit | just-bash-inproc | 1.811 | ±0.192 | - | ✓ |

### Strings

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| str_concat | bashkit | 0.070 | ±0.010 | - | ✓ |
| str_concat | bashkit-cli | 7.927 | ±0.131 | - | ✓ |
| str_concat | bashkit-js | 0.305 | ±0.089 | - | ✓ |
| str_concat | bashkit-py | 0.238 | ±0.082 | - | ✓ |
| str_concat | bash | 1.455 | ±0.123 | - | ✓ |
| str_concat | just-bash | 364.540 | ±13.668 | - | ✓ |
| str_concat | just-bash-inproc | 1.565 | ±0.168 | - | ✓ |
| str_printf | bashkit | 0.073 | ±0.034 | - | ✓ |
| str_printf | bashkit-cli | 7.821 | ±0.275 | - | ✓ |
| str_printf | bashkit-js | 0.214 | ±0.049 | - | ✓ |
| str_printf | bashkit-py | 0.183 | ±0.017 | - | ✓ |
| str_printf | bash | 1.313 | ±0.058 | - | ✓ |
| str_printf | just-bash | 370.813 | ±15.943 | - | ✓ |
| str_printf | just-bash-inproc | 1.639 | ±0.191 | - | ✓ |
| str_printf_pad | bashkit | 0.058 | ±0.008 | - | ✓ |
| str_printf_pad | bashkit-cli | 8.309 | ±0.477 | - | ✓ |
| str_printf_pad | bashkit-js | 0.337 | ±0.040 | - | ✓ |
| str_printf_pad | bashkit-py | 0.179 | ±0.026 | - | ✓ |
| str_printf_pad | bash | 1.406 | ±0.051 | - | ✓ |
| str_printf_pad | just-bash | 386.739 | ±19.478 | - | ✓ |
| str_printf_pad | just-bash-inproc | 1.536 | ±0.145 | - | ✓ |
| str_echo_escape | bashkit | 0.065 | ±0.011 | - | ✓ |
| str_echo_escape | bashkit-cli | 8.241 | ±0.373 | - | ✓ |
| str_echo_escape | bashkit-js | 0.276 | ±0.048 | - | ✓ |
| str_echo_escape | bashkit-py | 0.248 | ±0.031 | - | ✓ |
| str_echo_escape | bash | 1.409 | ±0.094 | - | ✓ |
| str_echo_escape | just-bash | 360.284 | ±6.454 | - | ✓ |
| str_echo_escape | just-bash-inproc | 1.644 | ±0.243 | - | ✓ |
| str_prefix_strip | bashkit | 0.076 | ±0.017 | - | ✓ |
| str_prefix_strip | bashkit-cli | 7.984 | ±0.350 | - | ✓ |
| str_prefix_strip | bashkit-js | 1.589 | ±3.863 | - | ✓ |
| str_prefix_strip | bashkit-py | 0.186 | ±0.015 | - | ✓ |
| str_prefix_strip | bash | 1.547 | ±0.129 | - | ✓ |
| str_prefix_strip | just-bash | 371.112 | ±10.056 | - | ✓ |
| str_prefix_strip | just-bash-inproc | 1.768 | ±0.189 | - | ✓ |
| str_suffix_strip | bashkit | 0.068 | ±0.010 | - | ✓ |
| str_suffix_strip | bashkit-cli | 7.978 | ±0.410 | - | ✓ |
| str_suffix_strip | bashkit-js | 0.304 | ±0.071 | - | ✓ |
| str_suffix_strip | bashkit-py | 0.267 | ±0.089 | - | ✓ |
| str_suffix_strip | bash | 1.453 | ±0.112 | - | ✓ |
| str_suffix_strip | just-bash | 367.854 | ±6.246 | - | ✓ |
| str_suffix_strip | just-bash-inproc | 2.318 | ±0.424 | - | ✓ |
| str_uppercase | bashkit | 0.062 | ±0.004 | - | ✓ |
| str_uppercase | bashkit-cli | 8.434 | ±0.647 | - | ✓ |
| str_uppercase | bashkit-js | 0.267 | ±0.061 | - | ✓ |
| str_uppercase | bashkit-py | 0.233 | ±0.018 | - | ✓ |
| str_uppercase | bash | 1.471 | ±0.205 | - | ✓ |
| str_uppercase | just-bash | 377.215 | ±13.694 | - | ✓ |
| str_uppercase | just-bash-inproc | 1.597 | ±0.148 | - | ✓ |
| str_lowercase | bashkit | 0.063 | ±0.013 | - | ✓ |
| str_lowercase | bashkit-cli | 7.889 | ±0.340 | - | ✓ |
| str_lowercase | bashkit-js | 0.289 | ±0.101 | - | ✓ |
| str_lowercase | bashkit-py | 0.259 | ±0.055 | - | ✓ |
| str_lowercase | bash | 1.455 | ±0.066 | - | ✓ |
| str_lowercase | just-bash | 395.589 | ±39.656 | - | ✓ |
| str_lowercase | just-bash-inproc | 1.518 | ±0.216 | - | ✓ |

### Subshell

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| subshell_simple | bashkit | 0.058 | ±0.014 | - | ✓ |
| subshell_simple | bashkit-cli | 7.824 | ±0.187 | - | ✓ |
| subshell_simple | bashkit-js | 0.245 | ±0.048 | - | ✓ |
| subshell_simple | bashkit-py | 0.200 | ±0.023 | - | ✓ |
| subshell_simple | bash | 1.831 | ±0.059 | - | ✓ |
| subshell_simple | just-bash | 362.180 | ±6.615 | - | ✓ |
| subshell_simple | just-bash-inproc | 1.382 | ±0.226 | - | ✓ |
| subshell_isolation | bashkit | 0.068 | ±0.010 | - | ✓ |
| subshell_isolation | bashkit-cli | 8.176 | ±0.502 | - | ✓ |
| subshell_isolation | bashkit-js | 0.263 | ±0.044 | - | ✓ |
| subshell_isolation | bashkit-py | 0.219 | ±0.021 | - | ✓ |
| subshell_isolation | bash | 1.865 | ±0.089 | - | ✓ |
| subshell_isolation | just-bash | 357.979 | ±2.757 | - | ✓ |
| subshell_isolation | just-bash-inproc | 1.507 | ±0.079 | - | ✓ |
| subshell_nested | bashkit | 0.069 | ±0.005 | - | ✓ |
| subshell_nested | bashkit-cli | 7.927 | ±0.453 | - | ✓ |
| subshell_nested | bashkit-js | 0.276 | ±0.035 | - | ✓ |
| subshell_nested | bashkit-py | 0.252 | ±0.049 | - | ✓ |
| subshell_nested | bash | 3.236 | ±0.185 | - | ✓ |
| subshell_nested | just-bash | 363.953 | ±15.984 | - | ✓ |
| subshell_nested | just-bash-inproc | 1.721 | ±0.318 | - | ✓ |
| subshell_pipeline | bashkit | 0.063 | ±0.009 | - | ✓ |
| subshell_pipeline | bashkit-cli | 7.977 | ±0.562 | - | ✓ |
| subshell_pipeline | bashkit-js | 0.324 | ±0.111 | - | ✓ |
| subshell_pipeline | bashkit-py | 0.271 | ±0.085 | - | ✓ |
| subshell_pipeline | bash | 3.221 | ±0.215 | - | ✓ |
| subshell_pipeline | just-bash | 364.971 | ±13.282 | - | ✓ |
| subshell_pipeline | just-bash-inproc | 1.536 | ±0.388 | - | ✓ |
| subshell_capture_loop | bashkit | 0.113 | ±0.013 | - | ✓ |
| subshell_capture_loop | bashkit-cli | 7.683 | ±0.186 | - | ✓ |
| subshell_capture_loop | bashkit-js | 0.321 | ±0.085 | - | ✓ |
| subshell_capture_loop | bashkit-py | 0.257 | ±0.043 | - | ✓ |
| subshell_capture_loop | bash | 3.579 | ±0.134 | - | ✓ |
| subshell_capture_loop | just-bash | 365.032 | ±3.203 | - | ✓ |
| subshell_capture_loop | just-bash-inproc | 2.326 | ±0.197 | - | ✓ |
| subshell_process_subst | bashkit | 0.089 | ±0.014 | - | ✓ |
| subshell_process_subst | bashkit-cli | 7.827 | ±0.159 | - | ✓ |
| subshell_process_subst | bashkit-js | 0.259 | ±0.025 | - | ✓ |
| subshell_process_subst | bashkit-py | 0.224 | ±0.036 | - | ✓ |
| subshell_process_subst | bash | 2.211 | ±0.058 | - | ✓ |
| subshell_process_subst | just-bash | 366.594 | ±5.791 | - | ✓ |
| subshell_process_subst | just-bash-inproc | 2.041 | ±0.412 | - | ✓ |

### Tools

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| tool_grep_simple | bashkit | 0.071 | ±0.011 | - | ✓ |
| tool_grep_simple | bashkit-cli | 7.895 | ±0.208 | - | ✓ |
| tool_grep_simple | bashkit-js | 0.271 | ±0.052 | - | ✓ |
| tool_grep_simple | bashkit-py | 0.225 | ±0.036 | - | ✓ |
| tool_grep_simple | bash | 3.045 | ±0.132 | - | ✓ |
| tool_grep_simple | just-bash | 364.151 | ±2.692 | - | ✓ |
| tool_grep_simple | just-bash-inproc | 1.790 | ±0.355 | - | ✓ |
| tool_grep_case | bashkit | 0.173 | ±0.032 | - | ✓ |
| tool_grep_case | bashkit-cli | 7.999 | ±0.223 | - | ✓ |
| tool_grep_case | bashkit-js | 0.393 | ±0.065 | - | ✓ |
| tool_grep_case | bashkit-py | 0.386 | ±0.029 | - | ✓ |
| tool_grep_case | bash | 2.994 | ±0.106 | - | ✓ |
| tool_grep_case | just-bash | 359.800 | ±3.226 | - | ✓ |
| tool_grep_case | just-bash-inproc | 1.886 | ±0.246 | - | ✓ |
| tool_grep_count | bashkit | 0.066 | ±0.007 | - | ✓ |
| tool_grep_count | bashkit-cli | 7.998 | ±0.186 | - | ✓ |
| tool_grep_count | bashkit-js | 0.308 | ±0.066 | - | ✓ |
| tool_grep_count | bashkit-py | 0.229 | ±0.020 | - | ✓ |
| tool_grep_count | bash | 2.932 | ±0.143 | - | ✓ |
| tool_grep_count | just-bash | 362.769 | ±4.468 | - | ✓ |
| tool_grep_count | just-bash-inproc | 1.661 | ±0.127 | - | ✓ |
| tool_grep_invert | bashkit | 0.069 | ±0.011 | - | ✓ |
| tool_grep_invert | bashkit-cli | 7.833 | ±0.183 | - | ✓ |
| tool_grep_invert | bashkit-js | 0.284 | ±0.101 | - | ✓ |
| tool_grep_invert | bashkit-py | 0.209 | ±0.036 | - | ✓ |
| tool_grep_invert | bash | 2.976 | ±0.142 | - | ✓ |
| tool_grep_invert | just-bash | 359.290 | ±4.121 | - | ✓ |
| tool_grep_invert | just-bash-inproc | 1.734 | ±0.278 | - | ✓ |
| tool_grep_regex | bashkit | 0.106 | ±0.015 | - | ✓ |
| tool_grep_regex | bashkit-cli | 7.864 | ±0.182 | - | ✓ |
| tool_grep_regex | bashkit-js | 0.314 | ±0.056 | - | ✓ |
| tool_grep_regex | bashkit-py | 0.292 | ±0.081 | - | ✓ |
| tool_grep_regex | bash | 2.981 | ±0.193 | - | ✓ |
| tool_grep_regex | just-bash | 367.854 | ±8.965 | - | ✓ |
| tool_grep_regex | just-bash-inproc | 1.890 | ±0.278 | - | ✓ |
| tool_sed_replace | bashkit | 0.249 | ±0.056 | - | ✓ |
| tool_sed_replace | bashkit-cli | 9.083 | ±1.605 | - | ✓ |
| tool_sed_replace | bashkit-js | 0.442 | ±0.071 | - | ✓ |
| tool_sed_replace | bashkit-py | 0.346 | ±0.040 | - | ✓ |
| tool_sed_replace | bash | 3.076 | ±0.115 | - | ✓ |
| tool_sed_replace | just-bash | 369.366 | ±7.868 | - | ✓ |
| tool_sed_replace | just-bash-inproc | 1.933 | ±0.273 | - | ✓ |
| tool_sed_global | bashkit | 0.189 | ±0.050 | - | ✓ |
| tool_sed_global | bashkit-cli | 8.350 | ±0.291 | - | ✓ |
| tool_sed_global | bashkit-js | 0.414 | ±0.065 | - | ✓ |
| tool_sed_global | bashkit-py | 0.346 | ±0.038 | - | ✓ |
| tool_sed_global | bash | 3.030 | ±0.111 | - | ✓ |
| tool_sed_global | just-bash | 364.514 | ±5.747 | - | ✓ |
| tool_sed_global | just-bash-inproc | 1.833 | ±0.227 | - | ✓ |
| tool_sed_delete | bashkit | 0.102 | ±0.048 | - | ✓ |
| tool_sed_delete | bashkit-cli | 7.929 | ±0.224 | - | ✓ |
| tool_sed_delete | bashkit-js | 0.236 | ±0.036 | - | ✓ |
| tool_sed_delete | bashkit-py | 0.210 | ±0.026 | - | ✓ |
| tool_sed_delete | bash | 2.998 | ±0.064 | - | ✓ |
| tool_sed_delete | just-bash | 373.724 | ±17.259 | - | ✓ |
| tool_sed_delete | just-bash-inproc | 1.801 | ±0.165 | - | ✓ |
| tool_sed_lines | bashkit | 0.073 | ±0.018 | - | ✓ |
| tool_sed_lines | bashkit-cli | 7.718 | ±0.131 | - | ✓ |
| tool_sed_lines | bashkit-js | 0.248 | ±0.041 | - | ✓ |
| tool_sed_lines | bashkit-py | 0.248 | ±0.058 | - | ✓ |
| tool_sed_lines | bash | 3.046 | ±0.108 | - | ✓ |
| tool_sed_lines | just-bash | 359.065 | ±3.776 | - | ✓ |
| tool_sed_lines | just-bash-inproc | 1.636 | ±0.170 | - | ✓ |
| tool_sed_backrefs | bashkit | 0.214 | ±0.030 | - | ✓ |
| tool_sed_backrefs | bashkit-cli | 8.055 | ±0.097 | - | ✓ |
| tool_sed_backrefs | bashkit-js | 0.497 | ±0.033 | - | ✓ |
| tool_sed_backrefs | bashkit-py | 0.493 | ±0.042 | - | ✓ |
| tool_sed_backrefs | bash | 2.999 | ±0.095 | - | ✓ |
| tool_sed_backrefs | just-bash | 361.049 | ±3.005 | - | ✓ |
| tool_sed_backrefs | just-bash-inproc | 1.791 | ±0.320 | - | ✓ |
| tool_awk_print | bashkit | 0.064 | ±0.010 | - | ✓ |
| tool_awk_print | bashkit-cli | 7.717 | ±0.414 | - | ✓ |
| tool_awk_print | bashkit-js | 0.251 | ±0.051 | - | ✓ |
| tool_awk_print | bashkit-py | 0.214 | ±0.040 | - | ✓ |
| tool_awk_print | bash | 2.821 | ±0.122 | - | ✓ |
| tool_awk_print | just-bash | 368.708 | ±15.238 | - | ✓ |
| tool_awk_print | just-bash-inproc | 1.698 | ±0.160 | - | ✓ |
| tool_awk_sum | bashkit | 0.069 | ±0.003 | - | ✓ |
| tool_awk_sum | bashkit-cli | 8.005 | ±0.689 | - | ✓ |
| tool_awk_sum | bashkit-js | 0.272 | ±0.030 | - | ✓ |
| tool_awk_sum | bashkit-py | 0.270 | ±0.046 | - | ✓ |
| tool_awk_sum | bash | 2.983 | ±0.179 | - | ✓ |
| tool_awk_sum | just-bash | 363.091 | ±5.526 | - | ✓ |
| tool_awk_sum | just-bash-inproc | 1.734 | ±0.172 | - | ✓ |
| tool_awk_pattern | bashkit | 0.116 | ±0.013 | - | ✓ |
| tool_awk_pattern | bashkit-cli | 8.011 | ±0.260 | - | ✓ |
| tool_awk_pattern | bashkit-js | 0.373 | ±0.064 | - | ✓ |
| tool_awk_pattern | bashkit-py | 0.291 | ±0.087 | - | ✓ |
| tool_awk_pattern | bash | 2.865 | ±0.077 | - | ✓ |
| tool_awk_pattern | just-bash | 384.045 | ±22.347 | - | ✓ |
| tool_awk_pattern | just-bash-inproc | 1.934 | ±0.208 | - | ✓ |
| tool_awk_fieldsep | bashkit | 0.069 | ±0.010 | - | ✓ |
| tool_awk_fieldsep | bashkit-cli | 7.852 | ±0.211 | - | ✓ |
| tool_awk_fieldsep | bashkit-js | 1.712 | ±4.394 | - | ✓ |
| tool_awk_fieldsep | bashkit-py | 0.205 | ±0.027 | - | ✓ |
| tool_awk_fieldsep | bash | 2.954 | ±0.228 | - | ✓ |
| tool_awk_fieldsep | just-bash | 365.190 | ±5.832 | - | ✓ |
| tool_awk_fieldsep | just-bash-inproc | 1.715 | ±0.140 | - | ✓ |
| tool_awk_nf | bashkit | 0.080 | ±0.009 | - | ✓ |
| tool_awk_nf | bashkit-cli | 7.742 | ±0.123 | - | ✓ |
| tool_awk_nf | bashkit-js | 0.244 | ±0.037 | - | ✓ |
| tool_awk_nf | bashkit-py | 0.277 | ±0.045 | - | ✓ |
| tool_awk_nf | bash | 2.838 | ±0.103 | - | ✓ |
| tool_awk_nf | just-bash | 364.891 | ±11.141 | - | ✓ |
| tool_awk_nf | just-bash-inproc | 1.566 | ±0.118 | - | ✓ |
| tool_awk_compute | bashkit | 0.083 | ±0.008 | - | ✓ |
| tool_awk_compute | bashkit-cli | 7.636 | ±0.125 | - | ✓ |
| tool_awk_compute | bashkit-js | 0.228 | ±0.046 | - | ✓ |
| tool_awk_compute | bashkit-py | 0.210 | ±0.024 | - | ✓ |
| tool_awk_compute | bash | 2.788 | ±0.088 | - | ✓ |
| tool_awk_compute | just-bash | 360.531 | ±4.410 | - | ✓ |
| tool_awk_compute | just-bash-inproc | 1.601 | ±0.127 | - | ✓ |
| tool_jq_identity | bashkit | 0.584 | ±0.034 | - | ✓ |
| tool_jq_identity | bashkit-cli | 8.391 | ±0.165 | - | ✓ |
| tool_jq_identity | bashkit-js | 0.843 | ±0.033 | - | ✓ |
| tool_jq_identity | bashkit-py | 0.871 | ±0.041 | - | ✓ |
| tool_jq_identity | bash | 4.118 | ±0.094 | - | ✓ |
| tool_jq_identity | just-bash | 362.329 | ±9.297 | - | ✓ |
| tool_jq_identity | just-bash-inproc | 1.712 | ±0.341 | - | ✓ |
| tool_jq_field | bashkit | 0.628 | ±0.048 | - | ✓ |
| tool_jq_field | bashkit-cli | 8.687 | ±0.201 | - | ✓ |
| tool_jq_field | bashkit-js | 0.956 | ±0.157 | - | ✓ |
| tool_jq_field | bashkit-py | 0.980 | ±0.351 | - | ✓ |
| tool_jq_field | bash | 4.128 | ±0.048 | - | ✓ |
| tool_jq_field | just-bash | 361.630 | ±4.989 | - | ✓ |
| tool_jq_field | just-bash-inproc | 1.613 | ±0.189 | - | ✓ |
| tool_jq_array | bashkit | 0.645 | ±0.090 | - | ✓ |
| tool_jq_array | bashkit-cli | 8.680 | ±0.561 | - | ✓ |
| tool_jq_array | bashkit-js | 1.072 | ±0.094 | - | ✓ |
| tool_jq_array | bashkit-py | 0.833 | ±0.038 | - | ✓ |
| tool_jq_array | bash | 4.311 | ±0.183 | - | ✓ |
| tool_jq_array | just-bash | 360.824 | ±5.692 | - | ✓ |
| tool_jq_array | just-bash-inproc | 1.579 | ±0.199 | - | ✓ |
| tool_jq_filter | bashkit | 0.614 | ±0.036 | - | ✓ |
| tool_jq_filter | bashkit-cli | 8.407 | ±0.228 | - | ✓ |
| tool_jq_filter | bashkit-js | 1.120 | ±0.142 | - | ✓ |
| tool_jq_filter | bashkit-py | 0.949 | ±0.100 | - | ✓ |
| tool_jq_filter | bash | 4.348 | ±0.168 | - | ✓ |
| tool_jq_filter | just-bash | 361.666 | ±4.071 | - | ✓ |
| tool_jq_filter | just-bash-inproc | 1.631 | ±0.297 | - | ✓ |
| tool_jq_map | bashkit | 0.600 | ±0.029 | - | ✓ |
| tool_jq_map | bashkit-cli | 8.306 | ±0.095 | - | ✓ |
| tool_jq_map | bashkit-js | 0.979 | ±0.096 | - | ✓ |
| tool_jq_map | bashkit-py | 0.843 | ±0.052 | - | ✓ |
| tool_jq_map | bash | 4.251 | ±0.148 | - | ✓ |
| tool_jq_map | just-bash | 361.963 | ±4.151 | - | ✓ |
| tool_jq_map | just-bash-inproc | 1.432 | ±0.134 | - | ✓ |

### Variables

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| var_assign_simple | bashkit | 0.057 | ±0.005 | - | ✓ |
| var_assign_simple | bashkit-cli | 7.698 | ±0.230 | - | ✓ |
| var_assign_simple | bashkit-js | 0.236 | ±0.046 | - | ✓ |
| var_assign_simple | bashkit-py | 0.213 | ±0.027 | - | ✓ |
| var_assign_simple | bash | 1.409 | ±0.115 | - | ✓ |
| var_assign_simple | just-bash | 357.243 | ±7.957 | - | ✓ |
| var_assign_simple | just-bash-inproc | 1.805 | ±0.325 | - | ✓ |
| var_assign_many | bashkit | 0.103 | ±0.029 | - | ✓ |
| var_assign_many | bashkit-cli | 7.769 | ±0.172 | - | ✓ |
| var_assign_many | bashkit-js | 0.267 | ±0.030 | - | ✓ |
| var_assign_many | bashkit-py | 0.237 | ±0.041 | - | ✓ |
| var_assign_many | bash | 1.308 | ±0.018 | - | ✓ |
| var_assign_many | just-bash | 362.153 | ±5.927 | - | ✓ |
| var_assign_many | just-bash-inproc | 2.094 | ±0.236 | - | ✓ |
| var_default | bashkit | 0.059 | ±0.006 | - | ✓ |
| var_default | bashkit-cli | 7.984 | ±0.306 | - | ✓ |
| var_default | bashkit-js | 0.286 | ±0.051 | - | ✓ |
| var_default | bashkit-py | 0.198 | ±0.048 | - | ✓ |
| var_default | bash | 1.332 | ±0.079 | - | ✓ |
| var_default | just-bash | 356.850 | ±5.804 | - | ✓ |
| var_default | just-bash-inproc | 2.099 | ±0.612 | - | ✓ |
| var_length | bashkit | 0.064 | ±0.006 | - | ✓ |
| var_length | bashkit-cli | 7.816 | ±0.142 | - | ✓ |
| var_length | bashkit-js | 0.243 | ±0.056 | - | ✓ |
| var_length | bashkit-py | 0.196 | ±0.022 | - | ✓ |
| var_length | bash | 1.335 | ±0.045 | - | ✓ |
| var_length | just-bash | 354.310 | ±3.632 | - | ✓ |
| var_length | just-bash-inproc | 1.636 | ±0.228 | - | ✓ |
| var_substring | bashkit | 0.061 | ±0.006 | - | ✓ |
| var_substring | bashkit-cli | 7.583 | ±0.149 | - | ✓ |
| var_substring | bashkit-js | 0.220 | ±0.034 | - | ✓ |
| var_substring | bashkit-py | 0.281 | ±0.061 | - | ✓ |
| var_substring | bash | 1.359 | ±0.057 | - | ✓ |
| var_substring | just-bash | 358.269 | ±5.252 | - | ✓ |
| var_substring | just-bash-inproc | 1.627 | ±0.164 | - | ✓ |
| var_replace | bashkit | 0.068 | ±0.013 | - | ✓ |
| var_replace | bashkit-cli | 7.641 | ±0.126 | - | ✓ |
| var_replace | bashkit-js | 1.542 | ±3.725 | - | ✓ |
| var_replace | bashkit-py | 0.185 | ±0.017 | - | ✓ |
| var_replace | bash | 1.422 | ±0.179 | - | ✓ |
| var_replace | just-bash | 360.511 | ±6.994 | - | ✓ |
| var_replace | just-bash-inproc | 1.811 | ±0.159 | - | ✓ |
| var_nested | bashkit | 0.062 | ±0.010 | - | ✓ |
| var_nested | bashkit-cli | 7.746 | ±0.189 | - | ✓ |
| var_nested | bashkit-js | 0.929 | ±2.164 | - | ✓ |
| var_nested | bashkit-py | 0.182 | ±0.017 | - | ✓ |
| var_nested | bash | 1.350 | ±0.099 | - | ✓ |
| var_nested | just-bash | 357.018 | ±3.715 | - | ✓ |
| var_nested | just-bash-inproc | 1.668 | ±0.211 | - | ✓ |
| var_export | bashkit | 0.064 | ±0.018 | - | ✓ |
| var_export | bashkit-cli | 7.622 | ±0.101 | - | ✓ |
| var_export | bashkit-js | 0.195 | ±0.036 | - | ✓ |
| var_export | bashkit-py | 0.190 | ±0.027 | - | ✓ |
| var_export | bash | 1.304 | ±0.051 | - | ✓ |
| var_export | just-bash | 365.647 | ±10.508 | - | ✓ |
| var_export | just-bash-inproc | 1.854 | ±0.405 | - | ✓ |

## Runner Descriptions

| Runner | Type | Description |
|--------|------|-------------|
| bashkit | in-process | Rust library call, no fork/exec |
| bashkit-cli | subprocess | bashkit binary, new process per run |
| bashkit-js | persistent child | Node.js + @everruns/bashkit, warm interpreter |
| bashkit-py | persistent child | Python + bashkit package, warm interpreter |
| bash | subprocess | /bin/bash, new process per run |
| just-bash | subprocess | just-bash CLI, new process per run |
| just-bash-inproc | persistent child | Node.js + just-bash library, warm interpreter |

## Assumptions & Notes

- Times measured in nanoseconds, displayed in milliseconds
- Prewarm phase runs first few cases to warm up JIT/compilation
- Per-benchmark warmup iterations excluded from timing
- Output match compares against bash output when available
- Errors include execution failures and exit code mismatches
- In-process: interpreter runs inside the benchmark process
- Subprocess: new process spawned per benchmark run
- Persistent child: long-lived child process, amortizes startup cost

