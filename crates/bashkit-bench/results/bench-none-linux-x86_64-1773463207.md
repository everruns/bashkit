# Bashkit Benchmark Report

## System Information

- **Moniker**: `none-linux-x86_64`
- **Hostname**: (none)
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 4
- **Timestamp**: 1773463207
- **Iterations**: 10
- **Warmup**: 2
- **Prewarm cases**: 3

## Summary

Benchmarked 96 cases across 7 runners.

| Runner | Total Time (ms) | Avg/Case (ms) | Errors | Error Rate | Output Match |
|--------|-----------------|---------------|--------|------------|-------------|
| bashkit | 34.25 | 0.357 | 0 | 0.0% | 100.0% |
| bashkit-cli | 800.15 | 8.335 | 0 | 0.0% | 100.0% |
| bashkit-js | 69.15 | 0.720 | 0 | 0.0% | 100.0% |
| bashkit-py | 52.93 | 0.551 | 0 | 0.0% | 100.0% |
| bash | 812.77 | 8.466 | 0 | 0.0% | 100.0% |
| just-bash | 35546.18 | 370.273 | 30 | 3.1% | 96.9% |
| just-bash-inproc | 436.26 | 4.544 | 0 | 0.0% | 100.0% |

## Performance Comparison

**Bashkit is 23.7x faster** than bash on average.

## Results by Category

### Arithmetic

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arith_basic | bashkit | 0.088 | ±0.028 | - | ✓ |
| arith_basic | bashkit-cli | 7.731 | ±0.137 | - | ✓ |
| arith_basic | bashkit-js | 0.301 | ±0.099 | - | ✓ |
| arith_basic | bashkit-py | 0.242 | ±0.023 | - | ✓ |
| arith_basic | bash | 1.428 | ±0.109 | - | ✓ |
| arith_basic | just-bash | 357.886 | ±5.324 | - | ✓ |
| arith_basic | just-bash-inproc | 1.403 | ±0.114 | - | ✓ |
| arith_complex | bashkit | 0.069 | ±0.022 | - | ✓ |
| arith_complex | bashkit-cli | 7.786 | ±0.155 | - | ✓ |
| arith_complex | bashkit-js | 0.239 | ±0.042 | - | ✓ |
| arith_complex | bashkit-py | 0.252 | ±0.067 | - | ✓ |
| arith_complex | bash | 1.408 | ±0.093 | - | ✓ |
| arith_complex | just-bash | 358.726 | ±5.114 | - | ✓ |
| arith_complex | just-bash-inproc | 1.576 | ±0.135 | - | ✓ |
| arith_variables | bashkit | 0.063 | ±0.005 | - | ✓ |
| arith_variables | bashkit-cli | 7.870 | ±0.246 | - | ✓ |
| arith_variables | bashkit-js | 0.445 | ±0.222 | - | ✓ |
| arith_variables | bashkit-py | 0.355 | ±0.085 | - | ✓ |
| arith_variables | bash | 1.564 | ±0.167 | - | ✓ |
| arith_variables | just-bash | 369.023 | ±7.923 | - | ✓ |
| arith_variables | just-bash-inproc | 1.656 | ±0.273 | - | ✓ |
| arith_increment | bashkit | 0.141 | ±0.163 | - | ✓ |
| arith_increment | bashkit-cli | 7.838 | ±0.186 | - | ✓ |
| arith_increment | bashkit-js | 0.345 | ±0.079 | - | ✓ |
| arith_increment | bashkit-py | 0.227 | ±0.037 | - | ✓ |
| arith_increment | bash | 1.441 | ±0.111 | - | ✓ |
| arith_increment | just-bash | 371.429 | ±9.093 | - | ✓ |
| arith_increment | just-bash-inproc | 1.701 | ±0.220 | - | ✓ |
| arith_modulo | bashkit | 0.066 | ±0.016 | - | ✓ |
| arith_modulo | bashkit-cli | 7.853 | ±0.176 | - | ✓ |
| arith_modulo | bashkit-js | 0.289 | ±0.083 | - | ✓ |
| arith_modulo | bashkit-py | 0.212 | ±0.038 | - | ✓ |
| arith_modulo | bash | 1.763 | ±0.405 | - | ✓ |
| arith_modulo | just-bash | 361.535 | ±2.892 | - | ✓ |
| arith_modulo | just-bash-inproc | 1.550 | ±0.118 | - | ✓ |
| arith_loop_sum | bashkit | 0.167 | ±0.088 | - | ✓ |
| arith_loop_sum | bashkit-cli | 8.015 | ±0.198 | - | ✓ |
| arith_loop_sum | bashkit-js | 0.300 | ±0.058 | - | ✓ |
| arith_loop_sum | bashkit-py | 0.305 | ±0.052 | - | ✓ |
| arith_loop_sum | bash | 1.488 | ±0.064 | - | ✓ |
| arith_loop_sum | just-bash | 370.676 | ±4.790 | - | ✓ |
| arith_loop_sum | just-bash-inproc | 2.540 | ±0.279 | - | ✓ |

### Arrays

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arr_create | bashkit | 0.063 | ±0.009 | - | ✓ |
| arr_create | bashkit-cli | 8.012 | ±0.316 | - | ✓ |
| arr_create | bashkit-js | 0.277 | ±0.084 | - | ✓ |
| arr_create | bashkit-py | 0.209 | ±0.026 | - | ✓ |
| arr_create | bash | 1.478 | ±0.121 | - | ✓ |
| arr_create | just-bash | 360.646 | ±2.894 | - | ✓ |
| arr_create | just-bash-inproc | 1.662 | ±0.232 | - | ✓ |
| arr_all | bashkit | 0.058 | ±0.004 | - | ✓ |
| arr_all | bashkit-cli | 8.220 | ±0.383 | - | ✓ |
| arr_all | bashkit-js | 0.252 | ±0.055 | - | ✓ |
| arr_all | bashkit-py | 0.182 | ±0.016 | - | ✓ |
| arr_all | bash | 1.403 | ±0.082 | - | ✓ |
| arr_all | just-bash | 361.490 | ±2.349 | - | ✓ |
| arr_all | just-bash-inproc | 1.581 | ±0.229 | - | ✓ |
| arr_length | bashkit | 0.100 | ±0.038 | - | ✓ |
| arr_length | bashkit-cli | 7.892 | ±0.205 | - | ✓ |
| arr_length | bashkit-js | 0.271 | ±0.054 | - | ✓ |
| arr_length | bashkit-py | 0.214 | ±0.023 | - | ✓ |
| arr_length | bash | 1.425 | ±0.116 | - | ✓ |
| arr_length | just-bash | 364.086 | ±6.421 | - | ✓ |
| arr_length | just-bash-inproc | 1.636 | ±0.131 | - | ✓ |
| arr_iterate | bashkit | 0.101 | ±0.040 | - | ✓ |
| arr_iterate | bashkit-cli | 8.265 | ±0.437 | - | ✓ |
| arr_iterate | bashkit-js | 0.324 | ±0.074 | - | ✓ |
| arr_iterate | bashkit-py | 0.346 | ±0.075 | - | ✓ |
| arr_iterate | bash | 1.553 | ±0.084 | - | ✓ |
| arr_iterate | just-bash | 368.881 | ±6.633 | - | ✓ |
| arr_iterate | just-bash-inproc | 2.096 | ±0.247 | - | ✓ |
| arr_slice | bashkit | 0.120 | ±0.050 | - | ✓ |
| arr_slice | bashkit-cli | 8.182 | ±0.671 | - | ✓ |
| arr_slice | bashkit-js | 0.249 | ±0.100 | - | ✓ |
| arr_slice | bashkit-py | 0.188 | ±0.016 | - | ✓ |
| arr_slice | bash | 1.468 | ±0.106 | - | ✓ |
| arr_slice | just-bash | 365.932 | ±6.711 | - | ✓ |
| arr_slice | just-bash-inproc | 1.717 | ±0.187 | - | ✓ |
| arr_assign_index | bashkit | 0.083 | ±0.007 | - | ✓ |
| arr_assign_index | bashkit-cli | 7.730 | ±0.202 | - | ✓ |
| arr_assign_index | bashkit-js | 0.233 | ±0.057 | - | ✓ |
| arr_assign_index | bashkit-py | 0.280 | ±0.051 | - | ✓ |
| arr_assign_index | bash | 1.410 | ±0.052 | - | ✓ |
| arr_assign_index | just-bash | 368.375 | ±8.356 | - | ✓ |
| arr_assign_index | just-bash-inproc | 2.236 | ±0.833 | - | ✓ |

### Complex

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| complex_fibonacci | bashkit | 2.557 | ±0.079 | - | ✓ |
| complex_fibonacci | bashkit-cli | 10.894 | ±0.271 | - | ✓ |
| complex_fibonacci | bashkit-js | 3.076 | ±0.182 | - | ✓ |
| complex_fibonacci | bashkit-py | 2.883 | ±0.024 | - | ✓ |
| complex_fibonacci | bash | 105.844 | ±3.056 | - | ✓ |
| complex_fibonacci | just-bash | 414.741 | ±7.413 | - | ✓ |
| complex_fibonacci | just-bash-inproc | 48.918 | ±10.082 | - | ✓ |
| complex_fibonacci_iter | bashkit | 0.205 | ±0.073 | - | ✓ |
| complex_fibonacci_iter | bashkit-cli | 7.821 | ±0.228 | - | ✓ |
| complex_fibonacci_iter | bashkit-js | 0.391 | ±0.045 | - | ✓ |
| complex_fibonacci_iter | bashkit-py | 0.400 | ±0.146 | - | ✓ |
| complex_fibonacci_iter | bash | 1.488 | ±0.108 | - | ✓ |
| complex_fibonacci_iter | just-bash | 365.764 | ±4.671 | - | ✓ |
| complex_fibonacci_iter | just-bash-inproc | 2.158 | ±0.204 | - | ✓ |
| complex_nested_subst | bashkit | 0.063 | ±0.006 | - | ✓ |
| complex_nested_subst | bashkit-cli | 8.024 | ±0.328 | - | ✓ |
| complex_nested_subst | bashkit-js | 0.270 | ±0.082 | - | ✓ |
| complex_nested_subst | bashkit-py | 0.226 | ±0.039 | - | ✓ |
| complex_nested_subst | bash | 2.766 | ±0.101 | - | ✓ |
| complex_nested_subst | just-bash | 364.435 | ±8.738 | - | ✓ |
| complex_nested_subst | just-bash-inproc | 1.873 | ±0.245 | - | ✓ |
| complex_loop_compute | bashkit | 0.148 | ±0.020 | - | ✓ |
| complex_loop_compute | bashkit-cli | 8.004 | ±0.349 | - | ✓ |
| complex_loop_compute | bashkit-js | 0.380 | ±0.042 | - | ✓ |
| complex_loop_compute | bashkit-py | 0.553 | ±0.093 | - | ✓ |
| complex_loop_compute | bash | 1.593 | ±0.098 | - | ✓ |
| complex_loop_compute | just-bash | 373.203 | ±2.760 | - | ✓ |
| complex_loop_compute | just-bash-inproc | 2.156 | ±0.170 | - | ✓ |
| complex_string_build | bashkit | 0.077 | ±0.003 | - | ✓ |
| complex_string_build | bashkit-cli | 7.848 | ±0.323 | - | ✓ |
| complex_string_build | bashkit-js | 0.329 | ±0.082 | - | ✓ |
| complex_string_build | bashkit-py | 0.285 | ±0.049 | - | ✓ |
| complex_string_build | bash | 1.547 | ±0.077 | - | ✓ |
| complex_string_build | just-bash | 378.416 | ±20.749 | - | ✓ |
| complex_string_build | just-bash-inproc | 1.745 | ±0.183 | - | ✓ |
| complex_json_transform | bashkit | 0.679 | ±0.048 | - | ✓ |
| complex_json_transform | bashkit-cli | 8.806 | ±0.300 | - | ✓ |
| complex_json_transform | bashkit-js | 0.949 | ±0.056 | - | ✓ |
| complex_json_transform | bashkit-py | 0.992 | ±0.041 | - | ✓ |
| complex_json_transform | bash | 4.346 | ±0.160 | - | ✓ |
| complex_json_transform | just-bash | 375.984 | ±9.715 | - | ✓ |
| complex_json_transform | just-bash-inproc | 1.649 | ±0.195 | - | ✓ |
| complex_pipeline_text | bashkit | 0.214 | ±0.028 | - | ✓ |
| complex_pipeline_text | bashkit-cli | 8.391 | ±0.301 | - | ✓ |
| complex_pipeline_text | bashkit-js | 0.510 | ±0.101 | - | ✓ |
| complex_pipeline_text | bashkit-py | 0.452 | ±0.043 | - | ✓ |
| complex_pipeline_text | bash | 3.216 | ±0.137 | - | ✓ |
| complex_pipeline_text | just-bash | 377.084 | ±5.751 | - | ✓ |
| complex_pipeline_text | just-bash-inproc | 2.312 | ±0.417 | - | ✓ |

### Control

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| ctrl_if_simple | bashkit | 0.081 | ±0.017 | - | ✓ |
| ctrl_if_simple | bashkit-cli | 8.034 | ±0.392 | - | ✓ |
| ctrl_if_simple | bashkit-js | 0.261 | ±0.041 | - | ✓ |
| ctrl_if_simple | bashkit-py | 0.287 | ±0.096 | - | ✓ |
| ctrl_if_simple | bash | 1.450 | ±0.117 | - | ✓ |
| ctrl_if_simple | just-bash | 360.151 | ±5.205 | - | ✓ |
| ctrl_if_simple | just-bash-inproc | 1.640 | ±0.243 | - | ✓ |
| ctrl_if_else | bashkit | 0.069 | ±0.012 | - | ✓ |
| ctrl_if_else | bashkit-cli | 8.146 | ±0.304 | - | ✓ |
| ctrl_if_else | bashkit-js | 0.289 | ±0.045 | - | ✓ |
| ctrl_if_else | bashkit-py | 0.310 | ±0.065 | - | ✓ |
| ctrl_if_else | bash | 1.545 | ±0.141 | - | ✓ |
| ctrl_if_else | just-bash | 362.369 | ±6.625 | - | ✓ |
| ctrl_if_else | just-bash-inproc | 1.585 | ±0.123 | - | ✓ |
| ctrl_for_list | bashkit | 0.136 | ±0.062 | - | ✓ |
| ctrl_for_list | bashkit-cli | 8.069 | ±0.469 | - | ✓ |
| ctrl_for_list | bashkit-js | 0.258 | ±0.057 | - | ✓ |
| ctrl_for_list | bashkit-py | 0.265 | ±0.047 | - | ✓ |
| ctrl_for_list | bash | 1.407 | ±0.081 | - | ✓ |
| ctrl_for_list | just-bash | 372.946 | ±7.531 | - | ✓ |
| ctrl_for_list | just-bash-inproc | 2.272 | ±0.205 | - | ✓ |
| ctrl_for_range | bashkit | 0.103 | ±0.006 | - | ✓ |
| ctrl_for_range | bashkit-cli | 7.932 | ±0.156 | - | ✓ |
| ctrl_for_range | bashkit-js | 0.305 | ±0.039 | - | ✓ |
| ctrl_for_range | bashkit-py | 0.280 | ±0.070 | - | ✓ |
| ctrl_for_range | bash | 1.444 | ±0.058 | - | ✓ |
| ctrl_for_range | just-bash | 365.067 | ±4.793 | - | ✓ |
| ctrl_for_range | just-bash-inproc | 2.111 | ±0.203 | - | ✓ |
| ctrl_while | bashkit | 0.124 | ±0.007 | - | ✓ |
| ctrl_while | bashkit-cli | 8.051 | ±0.375 | - | ✓ |
| ctrl_while | bashkit-js | 0.337 | ±0.056 | - | ✓ |
| ctrl_while | bashkit-py | 0.378 | ±0.053 | - | ✓ |
| ctrl_while | bash | 1.562 | ±0.150 | - | ✓ |
| ctrl_while | just-bash | 380.546 | ±6.059 | - | ✓ |
| ctrl_while | just-bash-inproc | 3.221 | ±0.427 | - | ✓ |
| ctrl_case | bashkit | 0.105 | ±0.047 | - | ✓ |
| ctrl_case | bashkit-cli | 8.221 | ±0.462 | - | ✓ |
| ctrl_case | bashkit-js | 0.363 | ±0.075 | - | ✓ |
| ctrl_case | bashkit-py | 0.364 | ±0.070 | - | ✓ |
| ctrl_case | bash | 1.451 | ±0.128 | - | ✓ |
| ctrl_case | just-bash | 378.180 | ±12.240 | - | ✓ |
| ctrl_case | just-bash-inproc | 2.004 | ±0.182 | - | ✓ |
| ctrl_function | bashkit | 0.114 | ±0.055 | - | ✓ |
| ctrl_function | bashkit-cli | 8.130 | ±0.257 | - | ✓ |
| ctrl_function | bashkit-js | 0.282 | ±0.061 | - | ✓ |
| ctrl_function | bashkit-py | 0.294 | ±0.067 | - | ✓ |
| ctrl_function | bash | 1.506 | ±0.110 | - | ✓ |
| ctrl_function | just-bash | 361.141 | ±2.850 | - | ✓ |
| ctrl_function | just-bash-inproc | 1.659 | ±0.225 | - | ✓ |
| ctrl_function_return | bashkit | 0.156 | ±0.080 | - | ✓ |
| ctrl_function_return | bashkit-cli | 7.950 | ±0.205 | - | ✓ |
| ctrl_function_return | bashkit-js | 0.338 | ±0.065 | - | ✓ |
| ctrl_function_return | bashkit-py | 0.327 | ±0.044 | - | ✓ |
| ctrl_function_return | bash | 2.129 | ±0.158 | - | ✓ |
| ctrl_function_return | just-bash | 365.863 | ±5.845 | - | ✓ |
| ctrl_function_return | just-bash-inproc | 1.943 | ±0.182 | - | ✓ |
| ctrl_nested_loops | bashkit | 0.146 | ±0.032 | - | ✓ |
| ctrl_nested_loops | bashkit-cli | 7.938 | ±0.152 | - | ✓ |
| ctrl_nested_loops | bashkit-js | 0.385 | ±0.071 | - | ✓ |
| ctrl_nested_loops | bashkit-py | 0.386 | ±0.066 | - | ✓ |
| ctrl_nested_loops | bash | 1.620 | ±0.100 | - | ✓ |
| ctrl_nested_loops | just-bash | 370.430 | ±13.092 | - | ✓ |
| ctrl_nested_loops | just-bash-inproc | 2.473 | ±0.266 | - | ✓ |

### Io

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| io_redirect_write | bashkit | 0.094 | ±0.041 | - | ✓ |
| io_redirect_write | bashkit-cli | 7.806 | ±0.320 | - | ✓ |
| io_redirect_write | bashkit-js | 0.256 | ±0.035 | - | ✓ |
| io_redirect_write | bashkit-py | 0.217 | ±0.038 | - | ✓ |
| io_redirect_write | bash | 3.572 | ±0.112 | - | ✓ |
| io_redirect_write | just-bash | 366.732 | ±9.347 | 10 | ✗ |
| io_redirect_write | just-bash-inproc | 1.706 | ±0.115 | - | ✓ |
| io_append | bashkit | 0.098 | ±0.027 | - | ✓ |
| io_append | bashkit-cli | 8.213 | ±1.014 | - | ✓ |
| io_append | bashkit-js | 0.331 | ±0.107 | - | ✓ |
| io_append | bashkit-py | 0.220 | ±0.039 | - | ✓ |
| io_append | bash | 3.706 | ±0.202 | - | ✓ |
| io_append | just-bash | 364.810 | ±5.882 | 10 | ✗ |
| io_append | just-bash-inproc | 2.143 | ±0.345 | - | ✓ |
| io_dev_null | bashkit | 0.070 | ±0.019 | - | ✓ |
| io_dev_null | bashkit-cli | 8.078 | ±0.236 | - | ✓ |
| io_dev_null | bashkit-js | 0.273 | ±0.043 | - | ✓ |
| io_dev_null | bashkit-py | 0.241 | ±0.052 | - | ✓ |
| io_dev_null | bash | 1.608 | ±0.257 | - | ✓ |
| io_dev_null | just-bash | 371.535 | ±9.019 | 10 | ✗ |
| io_dev_null | just-bash-inproc | 1.510 | ±0.204 | - | ✓ |
| io_stderr_redirect | bashkit | 0.067 | ±0.027 | - | ✓ |
| io_stderr_redirect | bashkit-cli | 8.118 | ±0.342 | - | ✓ |
| io_stderr_redirect | bashkit-js | 0.222 | ±0.034 | - | ✓ |
| io_stderr_redirect | bashkit-py | 0.200 | ±0.046 | - | ✓ |
| io_stderr_redirect | bash | 1.432 | ±0.144 | - | ✓ |
| io_stderr_redirect | just-bash | 379.253 | ±11.376 | - | ✓ |
| io_stderr_redirect | just-bash-inproc | 1.338 | ±0.105 | - | ✓ |
| io_read_lines | bashkit | 0.105 | ±0.014 | - | ✓ |
| io_read_lines | bashkit-cli | 7.969 | ±0.375 | - | ✓ |
| io_read_lines | bashkit-js | 0.330 | ±0.066 | - | ✓ |
| io_read_lines | bashkit-py | 0.272 | ±0.069 | - | ✓ |
| io_read_lines | bash | 1.403 | ±0.043 | - | ✓ |
| io_read_lines | just-bash | 380.316 | ±12.681 | - | ✓ |
| io_read_lines | just-bash-inproc | 2.120 | ±0.227 | - | ✓ |
| io_multiline_heredoc | bashkit | 0.077 | ±0.044 | - | ✓ |
| io_multiline_heredoc | bashkit-cli | 8.102 | ±0.285 | - | ✓ |
| io_multiline_heredoc | bashkit-js | 0.322 | ±0.096 | - | ✓ |
| io_multiline_heredoc | bashkit-py | 0.276 | ±0.068 | - | ✓ |
| io_multiline_heredoc | bash | 2.971 | ±0.176 | - | ✓ |
| io_multiline_heredoc | just-bash | 367.796 | ±8.757 | - | ✓ |
| io_multiline_heredoc | just-bash-inproc | 1.714 | ±0.225 | - | ✓ |

### Large

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| large_loop_1000 | bashkit | 4.991 | ±0.195 | - | ✓ |
| large_loop_1000 | bashkit-cli | 12.367 | ±0.615 | - | ✓ |
| large_loop_1000 | bashkit-js | 4.788 | ±0.432 | - | ✓ |
| large_loop_1000 | bashkit-py | 4.749 | ±0.367 | - | ✓ |
| large_loop_1000 | bash | 4.214 | ±0.135 | - | ✓ |
| large_loop_1000 | just-bash | 400.008 | ±3.159 | - | ✓ |
| large_loop_1000 | just-bash-inproc | 21.536 | ±0.860 | - | ✓ |
| large_string_append_100 | bashkit | 0.505 | ±0.082 | - | ✓ |
| large_string_append_100 | bashkit-cli | 8.625 | ±0.465 | - | ✓ |
| large_string_append_100 | bashkit-js | 0.663 | ±0.074 | - | ✓ |
| large_string_append_100 | bashkit-py | 0.662 | ±0.064 | - | ✓ |
| large_string_append_100 | bash | 1.730 | ±0.052 | - | ✓ |
| large_string_append_100 | just-bash | 378.259 | ±12.186 | - | ✓ |
| large_string_append_100 | just-bash-inproc | 3.838 | ±0.387 | - | ✓ |
| large_array_fill_200 | bashkit | 0.686 | ±0.031 | - | ✓ |
| large_array_fill_200 | bashkit-cli | 8.751 | ±0.367 | - | ✓ |
| large_array_fill_200 | bashkit-js | 0.938 | ±0.053 | - | ✓ |
| large_array_fill_200 | bashkit-py | 1.084 | ±0.188 | - | ✓ |
| large_array_fill_200 | bash | 1.943 | ±0.088 | - | ✓ |
| large_array_fill_200 | just-bash | 389.383 | ±6.183 | - | ✓ |
| large_array_fill_200 | just-bash-inproc | 17.196 | ±0.777 | - | ✓ |
| large_nested_loops | bashkit | 2.255 | ±0.106 | - | ✓ |
| large_nested_loops | bashkit-cli | 10.633 | ±1.376 | - | ✓ |
| large_nested_loops | bashkit-js | 2.538 | ±0.292 | - | ✓ |
| large_nested_loops | bashkit-py | 2.573 | ±0.425 | - | ✓ |
| large_nested_loops | bash | 2.648 | ±0.077 | - | ✓ |
| large_nested_loops | just-bash | 385.286 | ±4.389 | - | ✓ |
| large_nested_loops | just-bash-inproc | 10.592 | ±0.341 | - | ✓ |
| large_fibonacci_12 | bashkit | 6.467 | ±0.229 | - | ✓ |
| large_fibonacci_12 | bashkit-cli | 14.988 | ±0.295 | - | ✓ |
| large_fibonacci_12 | bashkit-js | 7.055 | ±0.190 | - | ✓ |
| large_fibonacci_12 | bashkit-py | 7.025 | ±0.209 | - | ✓ |
| large_fibonacci_12 | bash | 281.163 | ±9.585 | - | ✓ |
| large_fibonacci_12 | just-bash | 465.006 | ±5.543 | - | ✓ |
| large_fibonacci_12 | just-bash-inproc | 100.609 | ±11.417 | - | ✓ |
| large_function_calls_500 | bashkit | 4.001 | ±0.084 | - | ✓ |
| large_function_calls_500 | bashkit-cli | 11.492 | ±0.426 | - | ✓ |
| large_function_calls_500 | bashkit-js | 3.716 | ±0.058 | - | ✓ |
| large_function_calls_500 | bashkit-py | 3.644 | ±0.090 | - | ✓ |
| large_function_calls_500 | bash | 215.748 | ±5.058 | - | ✓ |
| large_function_calls_500 | just-bash | 434.800 | ±6.382 | - | ✓ |
| large_function_calls_500 | just-bash-inproc | 54.370 | ±2.986 | - | ✓ |
| large_multiline_script | bashkit | 0.475 | ±0.033 | - | ✓ |
| large_multiline_script | bashkit-cli | 8.442 | ±0.184 | - | ✓ |
| large_multiline_script | bashkit-js | 0.770 | ±0.077 | - | ✓ |
| large_multiline_script | bashkit-py | 0.786 | ±0.069 | - | ✓ |
| large_multiline_script | bash | 1.776 | ±0.091 | - | ✓ |
| large_multiline_script | just-bash | 382.329 | ±9.118 | - | ✓ |
| large_multiline_script | just-bash-inproc | 4.412 | ±0.397 | - | ✓ |
| large_pipeline_chain | bashkit | 0.872 | ±0.079 | - | ✓ |
| large_pipeline_chain | bashkit-cli | 9.204 | ±0.421 | - | ✓ |
| large_pipeline_chain | bashkit-js | 1.133 | ±0.090 | - | ✓ |
| large_pipeline_chain | bashkit-py | 1.200 | ±0.107 | - | ✓ |
| large_pipeline_chain | bash | 3.318 | ±0.095 | - | ✓ |
| large_pipeline_chain | just-bash | 410.032 | ±8.174 | - | ✓ |
| large_pipeline_chain | just-bash-inproc | 17.882 | ±2.818 | - | ✓ |
| large_assoc_array | bashkit | 0.090 | ±0.028 | - | ✓ |
| large_assoc_array | bashkit-cli | 8.038 | ±0.237 | - | ✓ |
| large_assoc_array | bashkit-js | 0.285 | ±0.059 | - | ✓ |
| large_assoc_array | bashkit-py | 0.344 | ±0.067 | - | ✓ |
| large_assoc_array | bash | 1.469 | ±0.079 | - | ✓ |
| large_assoc_array | just-bash | 362.380 | ±3.204 | - | ✓ |
| large_assoc_array | just-bash-inproc | 1.750 | ±0.273 | - | ✓ |

### Pipes

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| pipe_simple | bashkit | 0.080 | ±0.010 | - | ✓ |
| pipe_simple | bashkit-cli | 7.965 | ±0.250 | - | ✓ |
| pipe_simple | bashkit-js | 0.298 | ±0.040 | - | ✓ |
| pipe_simple | bashkit-py | 0.205 | ±0.034 | - | ✓ |
| pipe_simple | bash | 2.937 | ±0.059 | - | ✓ |
| pipe_simple | just-bash | 365.293 | ±8.837 | - | ✓ |
| pipe_simple | just-bash-inproc | 1.617 | ±0.163 | - | ✓ |
| pipe_multi | bashkit | 0.098 | ±0.028 | - | ✓ |
| pipe_multi | bashkit-cli | 7.855 | ±0.283 | - | ✓ |
| pipe_multi | bashkit-js | 0.258 | ±0.069 | - | ✓ |
| pipe_multi | bashkit-py | 0.184 | ±0.022 | - | ✓ |
| pipe_multi | bash | 3.043 | ±0.140 | - | ✓ |
| pipe_multi | just-bash | 363.220 | ±7.041 | - | ✓ |
| pipe_multi | just-bash-inproc | 1.641 | ±0.326 | - | ✓ |
| pipe_command_subst | bashkit | 0.092 | ±0.035 | - | ✓ |
| pipe_command_subst | bashkit-cli | 7.759 | ±0.208 | - | ✓ |
| pipe_command_subst | bashkit-js | 0.292 | ±0.058 | - | ✓ |
| pipe_command_subst | bashkit-py | 0.218 | ±0.023 | - | ✓ |
| pipe_command_subst | bash | 1.872 | ±0.099 | - | ✓ |
| pipe_command_subst | just-bash | 364.240 | ±7.656 | - | ✓ |
| pipe_command_subst | just-bash-inproc | 1.714 | ±0.205 | - | ✓ |
| pipe_heredoc | bashkit | 0.052 | ±0.008 | - | ✓ |
| pipe_heredoc | bashkit-cli | 7.799 | ±0.232 | - | ✓ |
| pipe_heredoc | bashkit-js | 0.227 | ±0.034 | - | ✓ |
| pipe_heredoc | bashkit-py | 0.179 | ±0.017 | - | ✓ |
| pipe_heredoc | bash | 2.800 | ±0.158 | - | ✓ |
| pipe_heredoc | just-bash | 357.603 | ±4.481 | - | ✓ |
| pipe_heredoc | just-bash-inproc | 1.658 | ±0.219 | - | ✓ |
| pipe_herestring | bashkit | 0.057 | ±0.016 | - | ✓ |
| pipe_herestring | bashkit-cli | 8.067 | ±0.340 | - | ✓ |
| pipe_herestring | bashkit-js | 0.273 | ±0.124 | - | ✓ |
| pipe_herestring | bashkit-py | 0.253 | ±0.048 | - | ✓ |
| pipe_herestring | bash | 2.813 | ±0.201 | - | ✓ |
| pipe_herestring | just-bash | 357.103 | ±3.908 | - | ✓ |
| pipe_herestring | just-bash-inproc | 1.438 | ±0.205 | - | ✓ |
| pipe_discard | bashkit | 0.059 | ±0.003 | - | ✓ |
| pipe_discard | bashkit-cli | 7.913 | ±0.223 | - | ✓ |
| pipe_discard | bashkit-js | 0.302 | ±0.090 | - | ✓ |
| pipe_discard | bashkit-py | 0.247 | ±0.065 | - | ✓ |
| pipe_discard | bash | 1.892 | ±0.079 | - | ✓ |
| pipe_discard | just-bash | 361.943 | ±4.426 | - | ✓ |
| pipe_discard | just-bash-inproc | 1.798 | ±0.315 | - | ✓ |

### Startup

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| startup_empty | bashkit | 0.051 | ±0.006 | - | ✓ |
| startup_empty | bashkit-cli | 7.869 | ±0.325 | - | ✓ |
| startup_empty | bashkit-js | 0.392 | ±0.084 | - | ✓ |
| startup_empty | bashkit-py | 0.278 | ±0.079 | - | ✓ |
| startup_empty | bash | 1.510 | ±0.113 | - | ✓ |
| startup_empty | just-bash | 359.676 | ±10.522 | - | ✓ |
| startup_empty | just-bash-inproc | 2.014 | ±0.885 | - | ✓ |
| startup_true | bashkit | 0.054 | ±0.012 | - | ✓ |
| startup_true | bashkit-cli | 7.791 | ±0.195 | - | ✓ |
| startup_true | bashkit-js | 1.381 | ±3.276 | - | ✓ |
| startup_true | bashkit-py | 0.206 | ±0.060 | - | ✓ |
| startup_true | bash | 1.454 | ±0.153 | - | ✓ |
| startup_true | just-bash | 361.427 | ±11.255 | - | ✓ |
| startup_true | just-bash-inproc | 1.540 | ±0.241 | - | ✓ |
| startup_echo | bashkit | 0.055 | ±0.007 | - | ✓ |
| startup_echo | bashkit-cli | 8.434 | ±0.936 | - | ✓ |
| startup_echo | bashkit-js | 0.273 | ±0.045 | - | ✓ |
| startup_echo | bashkit-py | 0.240 | ±0.100 | - | ✓ |
| startup_echo | bash | 1.468 | ±0.124 | - | ✓ |
| startup_echo | just-bash | 381.167 | ±9.872 | - | ✓ |
| startup_echo | just-bash-inproc | 1.792 | ±0.232 | - | ✓ |
| startup_exit | bashkit | 0.054 | ±0.007 | - | ✓ |
| startup_exit | bashkit-cli | 8.233 | ±0.376 | - | ✓ |
| startup_exit | bashkit-js | 3.003 | ±7.979 | - | ✓ |
| startup_exit | bashkit-py | 0.200 | ±0.034 | - | ✓ |
| startup_exit | bash | 1.541 | ±0.078 | - | ✓ |
| startup_exit | just-bash | 364.805 | ±15.556 | - | ✓ |
| startup_exit | just-bash-inproc | 1.732 | ±0.263 | - | ✓ |

### Strings

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| str_concat | bashkit | 0.084 | ±0.034 | - | ✓ |
| str_concat | bashkit-cli | 8.036 | ±0.226 | - | ✓ |
| str_concat | bashkit-js | 0.260 | ±0.051 | - | ✓ |
| str_concat | bashkit-py | 0.364 | ±0.099 | - | ✓ |
| str_concat | bash | 1.503 | ±0.093 | - | ✓ |
| str_concat | just-bash | 359.915 | ±7.212 | - | ✓ |
| str_concat | just-bash-inproc | 1.476 | ±0.165 | - | ✓ |
| str_printf | bashkit | 0.059 | ±0.011 | - | ✓ |
| str_printf | bashkit-cli | 8.177 | ±0.276 | - | ✓ |
| str_printf | bashkit-js | 0.282 | ±0.066 | - | ✓ |
| str_printf | bashkit-py | 0.246 | ±0.042 | - | ✓ |
| str_printf | bash | 1.510 | ±0.100 | - | ✓ |
| str_printf | just-bash | 363.481 | ±8.597 | - | ✓ |
| str_printf | just-bash-inproc | 1.431 | ±0.183 | - | ✓ |
| str_printf_pad | bashkit | 0.056 | ±0.007 | - | ✓ |
| str_printf_pad | bashkit-cli | 7.648 | ±0.233 | - | ✓ |
| str_printf_pad | bashkit-js | 0.212 | ±0.062 | - | ✓ |
| str_printf_pad | bashkit-py | 0.240 | ±0.067 | - | ✓ |
| str_printf_pad | bash | 1.401 | ±0.141 | - | ✓ |
| str_printf_pad | just-bash | 362.157 | ±5.106 | - | ✓ |
| str_printf_pad | just-bash-inproc | 1.486 | ±0.132 | - | ✓ |
| str_echo_escape | bashkit | 0.098 | ±0.037 | - | ✓ |
| str_echo_escape | bashkit-cli | 8.044 | ±0.224 | - | ✓ |
| str_echo_escape | bashkit-js | 1.437 | ±3.465 | - | ✓ |
| str_echo_escape | bashkit-py | 0.322 | ±0.076 | - | ✓ |
| str_echo_escape | bash | 1.436 | ±0.066 | - | ✓ |
| str_echo_escape | just-bash | 360.633 | ±6.880 | - | ✓ |
| str_echo_escape | just-bash-inproc | 1.457 | ±0.147 | - | ✓ |
| str_prefix_strip | bashkit | 0.099 | ±0.023 | - | ✓ |
| str_prefix_strip | bashkit-cli | 7.848 | ±0.330 | - | ✓ |
| str_prefix_strip | bashkit-js | 0.342 | ±0.101 | - | ✓ |
| str_prefix_strip | bashkit-py | 0.285 | ±0.046 | - | ✓ |
| str_prefix_strip | bash | 1.462 | ±0.076 | - | ✓ |
| str_prefix_strip | just-bash | 369.024 | ±6.379 | - | ✓ |
| str_prefix_strip | just-bash-inproc | 1.660 | ±0.114 | - | ✓ |
| str_suffix_strip | bashkit | 0.098 | ±0.030 | - | ✓ |
| str_suffix_strip | bashkit-cli | 7.933 | ±0.165 | - | ✓ |
| str_suffix_strip | bashkit-js | 0.326 | ±0.087 | - | ✓ |
| str_suffix_strip | bashkit-py | 0.213 | ±0.027 | - | ✓ |
| str_suffix_strip | bash | 1.364 | ±0.049 | - | ✓ |
| str_suffix_strip | just-bash | 366.570 | ±3.418 | - | ✓ |
| str_suffix_strip | just-bash-inproc | 1.862 | ±0.270 | - | ✓ |
| str_uppercase | bashkit | 0.117 | ±0.041 | - | ✓ |
| str_uppercase | bashkit-cli | 8.139 | ±0.378 | - | ✓ |
| str_uppercase | bashkit-js | 0.302 | ±0.081 | - | ✓ |
| str_uppercase | bashkit-py | 0.214 | ±0.035 | - | ✓ |
| str_uppercase | bash | 1.437 | ±0.062 | - | ✓ |
| str_uppercase | just-bash | 362.021 | ±4.503 | - | ✓ |
| str_uppercase | just-bash-inproc | 1.521 | ±0.136 | - | ✓ |
| str_lowercase | bashkit | 0.060 | ±0.012 | - | ✓ |
| str_lowercase | bashkit-cli | 8.114 | ±0.265 | - | ✓ |
| str_lowercase | bashkit-js | 0.348 | ±0.074 | - | ✓ |
| str_lowercase | bashkit-py | 0.250 | ±0.046 | - | ✓ |
| str_lowercase | bash | 1.499 | ±0.105 | - | ✓ |
| str_lowercase | just-bash | 367.492 | ±3.879 | - | ✓ |
| str_lowercase | just-bash-inproc | 1.503 | ±0.134 | - | ✓ |

### Subshell

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| subshell_simple | bashkit | 0.055 | ±0.007 | - | ✓ |
| subshell_simple | bashkit-cli | 7.823 | ±0.194 | - | ✓ |
| subshell_simple | bashkit-js | 0.219 | ±0.032 | - | ✓ |
| subshell_simple | bashkit-py | 0.293 | ±0.055 | - | ✓ |
| subshell_simple | bash | 1.985 | ±0.089 | - | ✓ |
| subshell_simple | just-bash | 365.909 | ±5.655 | - | ✓ |
| subshell_simple | just-bash-inproc | 1.370 | ±0.161 | - | ✓ |
| subshell_isolation | bashkit | 0.074 | ±0.009 | - | ✓ |
| subshell_isolation | bashkit-cli | 8.108 | ±0.341 | - | ✓ |
| subshell_isolation | bashkit-js | 0.279 | ±0.049 | - | ✓ |
| subshell_isolation | bashkit-py | 0.239 | ±0.075 | - | ✓ |
| subshell_isolation | bash | 1.946 | ±0.148 | - | ✓ |
| subshell_isolation | just-bash | 373.860 | ±12.960 | - | ✓ |
| subshell_isolation | just-bash-inproc | 1.652 | ±0.294 | - | ✓ |
| subshell_nested | bashkit | 0.080 | ±0.013 | - | ✓ |
| subshell_nested | bashkit-cli | 8.024 | ±0.397 | - | ✓ |
| subshell_nested | bashkit-js | 0.274 | ±0.048 | - | ✓ |
| subshell_nested | bashkit-py | 0.256 | ±0.027 | - | ✓ |
| subshell_nested | bash | 3.356 | ±0.122 | - | ✓ |
| subshell_nested | just-bash | 365.331 | ±12.888 | - | ✓ |
| subshell_nested | just-bash-inproc | 1.681 | ±0.185 | - | ✓ |
| subshell_pipeline | bashkit | 0.057 | ±0.003 | - | ✓ |
| subshell_pipeline | bashkit-cli | 7.977 | ±0.405 | - | ✓ |
| subshell_pipeline | bashkit-js | 0.300 | ±0.049 | - | ✓ |
| subshell_pipeline | bashkit-py | 0.269 | ±0.058 | - | ✓ |
| subshell_pipeline | bash | 3.264 | ±0.099 | - | ✓ |
| subshell_pipeline | just-bash | 363.156 | ±5.643 | - | ✓ |
| subshell_pipeline | just-bash-inproc | 1.505 | ±0.180 | - | ✓ |
| subshell_capture_loop | bashkit | 0.117 | ±0.011 | - | ✓ |
| subshell_capture_loop | bashkit-cli | 7.721 | ±0.254 | - | ✓ |
| subshell_capture_loop | bashkit-js | 1.849 | ±4.578 | - | ✓ |
| subshell_capture_loop | bashkit-py | 0.320 | ±0.080 | - | ✓ |
| subshell_capture_loop | bash | 3.510 | ±0.115 | - | ✓ |
| subshell_capture_loop | just-bash | 370.628 | ±4.428 | - | ✓ |
| subshell_capture_loop | just-bash-inproc | 2.203 | ±0.228 | - | ✓ |
| subshell_process_subst | bashkit | 0.091 | ±0.012 | - | ✓ |
| subshell_process_subst | bashkit-cli | 7.951 | ±0.226 | - | ✓ |
| subshell_process_subst | bashkit-js | 0.285 | ±0.034 | - | ✓ |
| subshell_process_subst | bashkit-py | 0.281 | ±0.074 | - | ✓ |
| subshell_process_subst | bash | 2.337 | ±0.120 | - | ✓ |
| subshell_process_subst | just-bash | 363.652 | ±3.304 | - | ✓ |
| subshell_process_subst | just-bash-inproc | 1.834 | ±0.081 | - | ✓ |

### Tools

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| tool_grep_simple | bashkit | 0.085 | ±0.040 | - | ✓ |
| tool_grep_simple | bashkit-cli | 7.880 | ±0.192 | - | ✓ |
| tool_grep_simple | bashkit-js | 0.328 | ±0.069 | - | ✓ |
| tool_grep_simple | bashkit-py | 0.359 | ±0.048 | - | ✓ |
| tool_grep_simple | bash | 3.064 | ±0.116 | - | ✓ |
| tool_grep_simple | just-bash | 367.410 | ±3.771 | - | ✓ |
| tool_grep_simple | just-bash-inproc | 2.019 | ±0.431 | - | ✓ |
| tool_grep_case | bashkit | 0.172 | ±0.021 | - | ✓ |
| tool_grep_case | bashkit-cli | 8.649 | ±0.272 | - | ✓ |
| tool_grep_case | bashkit-js | 0.472 | ±0.072 | - | ✓ |
| tool_grep_case | bashkit-py | 0.382 | ±0.064 | - | ✓ |
| tool_grep_case | bash | 3.283 | ±0.375 | - | ✓ |
| tool_grep_case | just-bash | 367.926 | ±5.948 | - | ✓ |
| tool_grep_case | just-bash-inproc | 1.825 | ±0.157 | - | ✓ |
| tool_grep_count | bashkit | 0.072 | ±0.015 | - | ✓ |
| tool_grep_count | bashkit-cli | 7.829 | ±0.125 | - | ✓ |
| tool_grep_count | bashkit-js | 0.419 | ±0.125 | - | ✓ |
| tool_grep_count | bashkit-py | 0.299 | ±0.067 | - | ✓ |
| tool_grep_count | bash | 3.113 | ±0.140 | - | ✓ |
| tool_grep_count | just-bash | 370.345 | ±4.980 | - | ✓ |
| tool_grep_count | just-bash-inproc | 1.688 | ±0.233 | - | ✓ |
| tool_grep_invert | bashkit | 0.066 | ±0.007 | - | ✓ |
| tool_grep_invert | bashkit-cli | 8.123 | ±0.314 | - | ✓ |
| tool_grep_invert | bashkit-js | 0.339 | ±0.095 | - | ✓ |
| tool_grep_invert | bashkit-py | 0.324 | ±0.045 | - | ✓ |
| tool_grep_invert | bash | 3.043 | ±0.084 | - | ✓ |
| tool_grep_invert | just-bash | 366.325 | ±7.830 | - | ✓ |
| tool_grep_invert | just-bash-inproc | 1.693 | ±0.186 | - | ✓ |
| tool_grep_regex | bashkit | 0.111 | ±0.019 | - | ✓ |
| tool_grep_regex | bashkit-cli | 8.165 | ±0.180 | - | ✓ |
| tool_grep_regex | bashkit-js | 0.363 | ±0.069 | - | ✓ |
| tool_grep_regex | bashkit-py | 0.340 | ±0.053 | - | ✓ |
| tool_grep_regex | bash | 3.066 | ±0.083 | - | ✓ |
| tool_grep_regex | just-bash | 363.834 | ±2.593 | - | ✓ |
| tool_grep_regex | just-bash-inproc | 1.963 | ±0.227 | - | ✓ |
| tool_sed_replace | bashkit | 0.185 | ±0.038 | - | ✓ |
| tool_sed_replace | bashkit-cli | 8.243 | ±0.178 | - | ✓ |
| tool_sed_replace | bashkit-js | 1.859 | ±3.897 | - | ✓ |
| tool_sed_replace | bashkit-py | 0.441 | ±0.058 | - | ✓ |
| tool_sed_replace | bash | 3.047 | ±0.083 | - | ✓ |
| tool_sed_replace | just-bash | 362.329 | ±3.313 | - | ✓ |
| tool_sed_replace | just-bash-inproc | 1.828 | ±0.277 | - | ✓ |
| tool_sed_global | bashkit | 0.174 | ±0.042 | - | ✓ |
| tool_sed_global | bashkit-cli | 8.329 | ±0.288 | - | ✓ |
| tool_sed_global | bashkit-js | 0.511 | ±0.066 | - | ✓ |
| tool_sed_global | bashkit-py | 0.420 | ±0.068 | - | ✓ |
| tool_sed_global | bash | 3.139 | ±0.123 | - | ✓ |
| tool_sed_global | just-bash | 361.819 | ±4.653 | - | ✓ |
| tool_sed_global | just-bash-inproc | 1.724 | ±0.157 | - | ✓ |
| tool_sed_delete | bashkit | 0.145 | ±0.127 | - | ✓ |
| tool_sed_delete | bashkit-cli | 7.913 | ±0.142 | - | ✓ |
| tool_sed_delete | bashkit-js | 0.292 | ±0.038 | - | ✓ |
| tool_sed_delete | bashkit-py | 0.219 | ±0.038 | - | ✓ |
| tool_sed_delete | bash | 3.093 | ±0.080 | - | ✓ |
| tool_sed_delete | just-bash | 364.722 | ±9.664 | - | ✓ |
| tool_sed_delete | just-bash-inproc | 1.881 | ±0.140 | - | ✓ |
| tool_sed_lines | bashkit | 0.059 | ±0.006 | - | ✓ |
| tool_sed_lines | bashkit-cli | 7.824 | ±0.236 | - | ✓ |
| tool_sed_lines | bashkit-js | 0.330 | ±0.089 | - | ✓ |
| tool_sed_lines | bashkit-py | 0.205 | ±0.016 | - | ✓ |
| tool_sed_lines | bash | 3.077 | ±0.089 | - | ✓ |
| tool_sed_lines | just-bash | 362.527 | ±4.453 | - | ✓ |
| tool_sed_lines | just-bash-inproc | 2.052 | ±0.506 | - | ✓ |
| tool_sed_backrefs | bashkit | 0.250 | ±0.039 | - | ✓ |
| tool_sed_backrefs | bashkit-cli | 9.024 | ±0.697 | - | ✓ |
| tool_sed_backrefs | bashkit-js | 0.485 | ±0.079 | - | ✓ |
| tool_sed_backrefs | bashkit-py | 0.552 | ±0.099 | - | ✓ |
| tool_sed_backrefs | bash | 3.257 | ±0.163 | - | ✓ |
| tool_sed_backrefs | just-bash | 378.981 | ±8.079 | - | ✓ |
| tool_sed_backrefs | just-bash-inproc | 1.991 | ±0.199 | - | ✓ |
| tool_awk_print | bashkit | 0.063 | ±0.014 | - | ✓ |
| tool_awk_print | bashkit-cli | 9.634 | ±1.520 | - | ✓ |
| tool_awk_print | bashkit-js | 0.299 | ±0.060 | - | ✓ |
| tool_awk_print | bashkit-py | 0.257 | ±0.072 | - | ✓ |
| tool_awk_print | bash | 3.216 | ±0.122 | - | ✓ |
| tool_awk_print | just-bash | 366.068 | ±7.139 | - | ✓ |
| tool_awk_print | just-bash-inproc | 2.232 | ±0.418 | - | ✓ |
| tool_awk_sum | bashkit | 0.073 | ±0.013 | - | ✓ |
| tool_awk_sum | bashkit-cli | 7.816 | ±0.195 | - | ✓ |
| tool_awk_sum | bashkit-js | 0.347 | ±0.062 | - | ✓ |
| tool_awk_sum | bashkit-py | 0.239 | ±0.037 | - | ✓ |
| tool_awk_sum | bash | 2.986 | ±0.191 | - | ✓ |
| tool_awk_sum | just-bash | 365.826 | ±4.823 | - | ✓ |
| tool_awk_sum | just-bash-inproc | 1.945 | ±0.273 | - | ✓ |
| tool_awk_pattern | bashkit | 0.090 | ±0.010 | - | ✓ |
| tool_awk_pattern | bashkit-cli | 8.454 | ±0.902 | - | ✓ |
| tool_awk_pattern | bashkit-js | 0.337 | ±0.081 | - | ✓ |
| tool_awk_pattern | bashkit-py | 0.349 | ±0.100 | - | ✓ |
| tool_awk_pattern | bash | 2.937 | ±0.117 | - | ✓ |
| tool_awk_pattern | just-bash | 369.597 | ±11.954 | - | ✓ |
| tool_awk_pattern | just-bash-inproc | 1.908 | ±0.192 | - | ✓ |
| tool_awk_fieldsep | bashkit | 0.062 | ±0.005 | - | ✓ |
| tool_awk_fieldsep | bashkit-cli | 8.187 | ±1.107 | - | ✓ |
| tool_awk_fieldsep | bashkit-js | 1.874 | ±4.775 | - | ✓ |
| tool_awk_fieldsep | bashkit-py | 0.249 | ±0.070 | - | ✓ |
| tool_awk_fieldsep | bash | 3.019 | ±0.281 | - | ✓ |
| tool_awk_fieldsep | just-bash | 364.741 | ±3.495 | - | ✓ |
| tool_awk_fieldsep | just-bash-inproc | 1.748 | ±0.099 | - | ✓ |
| tool_awk_nf | bashkit | 0.063 | ±0.012 | - | ✓ |
| tool_awk_nf | bashkit-cli | 8.676 | ±0.558 | - | ✓ |
| tool_awk_nf | bashkit-js | 0.232 | ±0.046 | - | ✓ |
| tool_awk_nf | bashkit-py | 0.203 | ±0.037 | - | ✓ |
| tool_awk_nf | bash | 2.840 | ±0.070 | - | ✓ |
| tool_awk_nf | just-bash | 366.102 | ±6.858 | - | ✓ |
| tool_awk_nf | just-bash-inproc | 1.693 | ±0.151 | - | ✓ |
| tool_awk_compute | bashkit | 0.062 | ±0.005 | - | ✓ |
| tool_awk_compute | bashkit-cli | 7.912 | ±0.362 | - | ✓ |
| tool_awk_compute | bashkit-js | 0.408 | ±0.140 | - | ✓ |
| tool_awk_compute | bashkit-py | 0.284 | ±0.142 | - | ✓ |
| tool_awk_compute | bash | 3.003 | ±0.210 | - | ✓ |
| tool_awk_compute | just-bash | 366.602 | ±8.958 | - | ✓ |
| tool_awk_compute | just-bash-inproc | 1.636 | ±0.219 | - | ✓ |
| tool_jq_identity | bashkit | 0.653 | ±0.060 | - | ✓ |
| tool_jq_identity | bashkit-cli | 8.820 | ±0.422 | - | ✓ |
| tool_jq_identity | bashkit-js | 0.860 | ±0.047 | - | ✓ |
| tool_jq_identity | bashkit-py | 1.002 | ±0.137 | - | ✓ |
| tool_jq_identity | bash | 4.332 | ±0.113 | - | ✓ |
| tool_jq_identity | just-bash | 368.711 | ±4.753 | - | ✓ |
| tool_jq_identity | just-bash-inproc | 1.630 | ±0.174 | - | ✓ |
| tool_jq_field | bashkit | 0.664 | ±0.041 | - | ✓ |
| tool_jq_field | bashkit-cli | 8.878 | ±0.429 | - | ✓ |
| tool_jq_field | bashkit-js | 1.039 | ±0.106 | - | ✓ |
| tool_jq_field | bashkit-py | 0.972 | ±0.113 | - | ✓ |
| tool_jq_field | bash | 4.166 | ±0.113 | - | ✓ |
| tool_jq_field | just-bash | 366.506 | ±5.497 | - | ✓ |
| tool_jq_field | just-bash-inproc | 1.746 | ±0.232 | - | ✓ |
| tool_jq_array | bashkit | 0.589 | ±0.023 | - | ✓ |
| tool_jq_array | bashkit-cli | 8.741 | ±0.424 | - | ✓ |
| tool_jq_array | bashkit-js | 0.977 | ±0.091 | - | ✓ |
| tool_jq_array | bashkit-py | 0.869 | ±0.090 | - | ✓ |
| tool_jq_array | bash | 4.273 | ±0.158 | - | ✓ |
| tool_jq_array | just-bash | 369.295 | ±16.187 | - | ✓ |
| tool_jq_array | just-bash-inproc | 1.522 | ±0.136 | - | ✓ |
| tool_jq_filter | bashkit | 0.611 | ±0.026 | - | ✓ |
| tool_jq_filter | bashkit-cli | 8.655 | ±0.340 | - | ✓ |
| tool_jq_filter | bashkit-js | 0.912 | ±0.045 | - | ✓ |
| tool_jq_filter | bashkit-py | 0.830 | ±0.053 | - | ✓ |
| tool_jq_filter | bash | 4.301 | ±0.162 | - | ✓ |
| tool_jq_filter | just-bash | 367.404 | ±4.380 | - | ✓ |
| tool_jq_filter | just-bash-inproc | 1.813 | ±0.268 | - | ✓ |
| tool_jq_map | bashkit | 0.600 | ±0.037 | - | ✓ |
| tool_jq_map | bashkit-cli | 8.569 | ±0.244 | - | ✓ |
| tool_jq_map | bashkit-js | 1.025 | ±0.088 | - | ✓ |
| tool_jq_map | bashkit-py | 0.960 | ±0.095 | - | ✓ |
| tool_jq_map | bash | 4.358 | ±0.177 | - | ✓ |
| tool_jq_map | just-bash | 365.756 | ±2.527 | - | ✓ |
| tool_jq_map | just-bash-inproc | 1.561 | ±0.191 | - | ✓ |

### Variables

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| var_assign_simple | bashkit | 0.059 | ±0.007 | - | ✓ |
| var_assign_simple | bashkit-cli | 8.185 | ±0.504 | - | ✓ |
| var_assign_simple | bashkit-js | 0.324 | ±0.151 | - | ✓ |
| var_assign_simple | bashkit-py | 0.262 | ±0.063 | - | ✓ |
| var_assign_simple | bash | 1.453 | ±0.091 | - | ✓ |
| var_assign_simple | just-bash | 367.761 | ±5.338 | - | ✓ |
| var_assign_simple | just-bash-inproc | 2.033 | ±0.338 | - | ✓ |
| var_assign_many | bashkit | 0.091 | ±0.009 | - | ✓ |
| var_assign_many | bashkit-cli | 8.229 | ±0.435 | - | ✓ |
| var_assign_many | bashkit-js | 0.344 | ±0.060 | - | ✓ |
| var_assign_many | bashkit-py | 0.340 | ±0.055 | - | ✓ |
| var_assign_many | bash | 1.481 | ±0.093 | - | ✓ |
| var_assign_many | just-bash | 373.489 | ±7.916 | - | ✓ |
| var_assign_many | just-bash-inproc | 2.593 | ±0.429 | - | ✓ |
| var_default | bashkit | 0.058 | ±0.007 | - | ✓ |
| var_default | bashkit-cli | 8.220 | ±0.676 | - | ✓ |
| var_default | bashkit-js | 0.318 | ±0.067 | - | ✓ |
| var_default | bashkit-py | 0.291 | ±0.069 | - | ✓ |
| var_default | bash | 1.451 | ±0.053 | - | ✓ |
| var_default | just-bash | 357.821 | ±3.284 | - | ✓ |
| var_default | just-bash-inproc | 1.636 | ±0.296 | - | ✓ |
| var_length | bashkit | 0.077 | ±0.017 | - | ✓ |
| var_length | bashkit-cli | 7.819 | ±0.207 | - | ✓ |
| var_length | bashkit-js | 0.289 | ±0.086 | - | ✓ |
| var_length | bashkit-py | 0.244 | ±0.040 | - | ✓ |
| var_length | bash | 1.407 | ±0.060 | - | ✓ |
| var_length | just-bash | 358.595 | ±3.028 | - | ✓ |
| var_length | just-bash-inproc | 1.517 | ±0.138 | - | ✓ |
| var_substring | bashkit | 0.120 | ±0.077 | - | ✓ |
| var_substring | bashkit-cli | 7.841 | ±0.335 | - | ✓ |
| var_substring | bashkit-js | 0.226 | ±0.038 | - | ✓ |
| var_substring | bashkit-py | 0.199 | ±0.021 | - | ✓ |
| var_substring | bash | 1.413 | ±0.153 | - | ✓ |
| var_substring | just-bash | 361.240 | ±3.606 | - | ✓ |
| var_substring | just-bash-inproc | 1.735 | ±0.209 | - | ✓ |
| var_replace | bashkit | 0.081 | ±0.029 | - | ✓ |
| var_replace | bashkit-cli | 7.791 | ±0.134 | - | ✓ |
| var_replace | bashkit-js | 3.489 | ±9.377 | - | ✓ |
| var_replace | bashkit-py | 0.192 | ±0.018 | - | ✓ |
| var_replace | bash | 1.385 | ±0.073 | - | ✓ |
| var_replace | just-bash | 368.950 | ±12.807 | - | ✓ |
| var_replace | just-bash-inproc | 2.054 | ±0.250 | - | ✓ |
| var_nested | bashkit | 0.097 | ±0.029 | - | ✓ |
| var_nested | bashkit-cli | 8.098 | ±0.387 | - | ✓ |
| var_nested | bashkit-js | 0.987 | ±2.302 | - | ✓ |
| var_nested | bashkit-py | 0.241 | ±0.027 | - | ✓ |
| var_nested | bash | 1.401 | ±0.082 | - | ✓ |
| var_nested | just-bash | 364.230 | ±4.951 | - | ✓ |
| var_nested | just-bash-inproc | 1.662 | ±0.181 | - | ✓ |
| var_export | bashkit | 0.076 | ±0.008 | - | ✓ |
| var_export | bashkit-cli | 7.933 | ±0.185 | - | ✓ |
| var_export | bashkit-js | 0.185 | ±0.044 | - | ✓ |
| var_export | bashkit-py | 0.214 | ±0.036 | - | ✓ |
| var_export | bash | 1.380 | ±0.075 | - | ✓ |
| var_export | just-bash | 358.030 | ±3.928 | - | ✓ |
| var_export | just-bash-inproc | 1.559 | ±0.114 | - | ✓ |

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

