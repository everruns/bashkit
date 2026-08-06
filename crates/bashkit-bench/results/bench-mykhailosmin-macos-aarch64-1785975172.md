# Bashkit Benchmark Report

## System Information

- **Moniker**: `mykhailosmin-macos-aarch64`
- **Hostname**: Mykhailos-Mini.lan
- **OS**: macos
- **Architecture**: aarch64
- **CPUs**: 12
- **Timestamp**: 1785975172
- **Iterations**: 10
- **Warmup**: 2
- **Prewarm cases**: 3

## Summary

Benchmarked 96 cases across 2 runners.

| Runner | Total Time (ms) | Avg/Case (ms) | Errors | Error Rate | Output Match |
|--------|-----------------|---------------|--------|------------|-------------|
| bashkit | 50.58 | 0.527 | 0 | 0.0% | 95.8% |
| bash | 3351.84 | 34.915 | 20 | 2.1% | 100.0% |

## Performance Comparison

**Bashkit is 66.3x faster** than bash on average.

## Results by Category

### Arithmetic

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arith_basic | bashkit | 0.047 | ±0.043 | - | ✓ |
| arith_basic | bash | 11.833 | ±7.702 | - | ✓ |
| arith_complex | bashkit | 0.051 | ±0.030 | - | ✓ |
| arith_complex | bash | 6.589 | ±3.560 | - | ✓ |
| arith_variables | bashkit | 0.020 | ±0.001 | - | ✓ |
| arith_variables | bash | 8.838 | ±5.153 | - | ✓ |
| arith_increment | bashkit | 0.142 | ±0.355 | - | ✓ |
| arith_increment | bash | 9.888 | ±3.258 | - | ✓ |
| arith_modulo | bashkit | 0.046 | ±0.007 | - | ✓ |
| arith_modulo | bash | 6.592 | ±3.534 | - | ✓ |
| arith_loop_sum | bashkit | 2.462 | ±7.261 | - | ✓ |
| arith_loop_sum | bash | 8.514 | ±4.484 | - | ✓ |

### Arrays

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arr_create | bashkit | 0.019 | ±0.001 | - | ✓ |
| arr_create | bash | 7.440 | ±3.381 | - | ✓ |
| arr_all | bashkit | 0.019 | ±0.001 | - | ✓ |
| arr_all | bash | 5.457 | ±2.946 | - | ✓ |
| arr_length | bashkit | 0.018 | ±0.001 | - | ✓ |
| arr_length | bash | 9.074 | ±4.784 | - | ✓ |
| arr_iterate | bashkit | 0.072 | ±0.037 | - | ✓ |
| arr_iterate | bash | 7.745 | ±5.876 | - | ✓ |
| arr_slice | bashkit | 0.030 | ±0.018 | - | ✓ |
| arr_slice | bash | 6.289 | ±1.616 | - | ✓ |
| arr_assign_index | bashkit | 0.020 | ±0.001 | - | ✓ |
| arr_assign_index | bash | 6.118 | ±3.239 | - | ✓ |

### Complex

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| complex_fibonacci | bashkit | 3.356 | ±1.934 | - | ✓ |
| complex_fibonacci | bash | 192.573 | ±39.435 | - | ✓ |
| complex_fibonacci_iter | bashkit | 0.055 | ±0.014 | - | ✓ |
| complex_fibonacci_iter | bash | 4.267 | ±2.244 | - | ✓ |
| complex_nested_subst | bashkit | 0.078 | ±0.052 | - | ✓ |
| complex_nested_subst | bash | 10.043 | ±3.156 | - | ✓ |
| complex_loop_compute | bashkit | 0.050 | ±0.001 | - | ✓ |
| complex_loop_compute | bash | 8.075 | ±5.651 | - | ✓ |
| complex_string_build | bashkit | 0.025 | ±0.001 | - | ✓ |
| complex_string_build | bash | 4.672 | ±3.285 | - | ✓ |
| complex_json_transform | bashkit | 0.248 | ±0.012 | - | ✓ |
| complex_json_transform | bash | 12.190 | ±3.440 | - | ✓ |
| complex_pipeline_text | bashkit | 0.068 | ±0.003 | - | ✓ |
| complex_pipeline_text | bash | 11.880 | ±4.765 | - | ✓ |

### Control

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| ctrl_if_simple | bashkit | 0.018 | ±0.001 | - | ✓ |
| ctrl_if_simple | bash | 7.616 | ±3.636 | - | ✓ |
| ctrl_if_else | bashkit | 0.020 | ±0.001 | - | ✓ |
| ctrl_if_else | bash | 13.184 | ±8.063 | - | ✓ |
| ctrl_for_list | bashkit | 0.056 | ±0.029 | - | ✓ |
| ctrl_for_list | bash | 6.790 | ±2.747 | - | ✓ |
| ctrl_for_range | bashkit | 0.074 | ±0.004 | - | ✓ |
| ctrl_for_range | bash | 10.580 | ±8.626 | - | ✓ |
| ctrl_while | bashkit | 0.049 | ±0.002 | - | ✓ |
| ctrl_while | bash | 8.485 | ±5.118 | - | ✓ |
| ctrl_case | bashkit | 0.021 | ±0.001 | - | ✓ |
| ctrl_case | bash | 4.695 | ±2.309 | - | ✓ |
| ctrl_function | bashkit | 0.021 | ±0.002 | - | ✓ |
| ctrl_function | bash | 6.677 | ±2.754 | - | ✓ |
| ctrl_function_return | bashkit | 0.026 | ±0.002 | - | ✓ |
| ctrl_function_return | bash | 8.239 | ±4.338 | - | ✓ |
| ctrl_nested_loops | bashkit | 0.040 | ±0.004 | - | ✓ |
| ctrl_nested_loops | bash | 14.515 | ±21.225 | - | ✓ |

### Io

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| io_redirect_write | bashkit | 0.031 | ±0.002 | - | ✓ |
| io_redirect_write | bash | 30.515 | ±13.023 | - | ✓ |
| io_append | bashkit | 0.040 | ±0.006 | - | ✓ |
| io_append | bash | 11.167 | ±4.241 | - | ✓ |
| io_dev_null | bashkit | 0.018 | ±0.001 | - | ✓ |
| io_dev_null | bash | 4.252 | ±2.416 | - | ✓ |
| io_stderr_redirect | bashkit | 0.018 | ±0.001 | - | ✓ |
| io_stderr_redirect | bash | 8.458 | ±2.933 | - | ✓ |
| io_read_lines | bashkit | 0.050 | ±0.020 | - | ✓ |
| io_read_lines | bash | 7.903 | ±2.437 | - | ✓ |
| io_multiline_heredoc | bashkit | 0.118 | ±0.119 | - | ✓ |
| io_multiline_heredoc | bash | 9.570 | ±1.853 | - | ✓ |

### Large

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| large_loop_1000 | bashkit | 2.988 | ±1.128 | - | ✓ |
| large_loop_1000 | bash | 12.302 | ±5.935 | - | ✓ |
| large_string_append_100 | bashkit | 0.180 | ±0.002 | - | ✓ |
| large_string_append_100 | bash | 6.234 | ±1.817 | - | ✓ |
| large_array_fill_200 | bashkit | 0.775 | ±0.013 | - | ✓ |
| large_array_fill_200 | bash | 10.175 | ±4.574 | - | ✓ |
| large_nested_loops | bashkit | 1.327 | ±0.472 | - | ✓ |
| large_nested_loops | bash | 12.297 | ±8.177 | - | ✓ |
| large_fibonacci_12 | bashkit | 20.147 | ±16.629 | - | ✓ |
| large_fibonacci_12 | bash | 1263.744 | ±562.827 | - | ✓ |
| large_function_calls_500 | bashkit | 10.035 | ±8.573 | - | ✓ |
| large_function_calls_500 | bash | 1032.523 | ±403.901 | - | ✓ |
| large_multiline_script | bashkit | 0.220 | ±0.027 | - | ✗ |
| large_multiline_script | bash | 4.159 | ±0.937 | - | ✓ |
| large_pipeline_chain | bashkit | 0.468 | ±0.007 | - | ✓ |
| large_pipeline_chain | bash | 7.702 | ±1.396 | - | ✓ |
| large_assoc_array | bashkit | 0.023 | ±0.001 | - | ✗ |
| large_assoc_array | bash | 3.870 | ±1.609 | - | ✓ |

### Pipes

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| pipe_simple | bashkit | 0.024 | ±0.002 | - | ✓ |
| pipe_simple | bash | 11.578 | ±5.362 | - | ✓ |
| pipe_multi | bashkit | 0.035 | ±0.001 | - | ✓ |
| pipe_multi | bash | 11.292 | ±3.853 | - | ✓ |
| pipe_command_subst | bashkit | 0.021 | ±0.002 | - | ✓ |
| pipe_command_subst | bash | 3.927 | ±1.124 | - | ✓ |
| pipe_heredoc | bashkit | 0.022 | ±0.001 | - | ✓ |
| pipe_heredoc | bash | 8.089 | ±4.187 | - | ✓ |
| pipe_herestring | bashkit | 0.021 | ±0.001 | - | ✓ |
| pipe_herestring | bash | 6.274 | ±1.308 | - | ✓ |
| pipe_discard | bashkit | 0.020 | ±0.001 | - | ✓ |
| pipe_discard | bash | 6.603 | ±3.417 | - | ✓ |

### Startup

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| startup_empty | bashkit | 0.015 | ±0.001 | - | ✓ |
| startup_empty | bash | 5.180 | ±3.605 | - | ✓ |
| startup_true | bashkit | 0.018 | ±0.006 | - | ✓ |
| startup_true | bash | 9.358 | ±5.640 | - | ✓ |
| startup_echo | bashkit | 0.016 | ±0.001 | - | ✓ |
| startup_echo | bash | 5.336 | ±2.543 | - | ✓ |
| startup_exit | bashkit | 0.039 | ±0.002 | - | ✓ |
| startup_exit | bash | 5.481 | ±5.334 | - | ✓ |

### Strings

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| str_concat | bashkit | 0.019 | ±0.001 | - | ✓ |
| str_concat | bash | 9.470 | ±4.095 | - | ✓ |
| str_printf | bashkit | 0.017 | ±0.001 | - | ✓ |
| str_printf | bash | 6.343 | ±7.660 | - | ✓ |
| str_printf_pad | bashkit | 0.040 | ±0.002 | - | ✓ |
| str_printf_pad | bash | 6.030 | ±2.633 | - | ✓ |
| str_echo_escape | bashkit | 0.016 | ±0.001 | - | ✓ |
| str_echo_escape | bash | 5.497 | ±1.638 | - | ✓ |
| str_prefix_strip | bashkit | 0.040 | ±0.021 | - | ✓ |
| str_prefix_strip | bash | 8.906 | ±7.406 | - | ✓ |
| str_suffix_strip | bashkit | 0.020 | ±0.005 | - | ✓ |
| str_suffix_strip | bash | 5.320 | ±3.266 | - | ✓ |
| str_uppercase | bashkit | 0.018 | ±0.001 | - | ✗ |
| str_uppercase | bash | 7.271 | ±3.867 | 10 | ✓ |
| str_lowercase | bashkit | 0.100 | ±0.129 | - | ✗ |
| str_lowercase | bash | 7.972 | ±5.049 | 10 | ✓ |

### Subshell

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| subshell_simple | bashkit | 0.017 | ±0.001 | - | ✓ |
| subshell_simple | bash | 3.553 | ±0.599 | - | ✓ |
| subshell_isolation | bashkit | 0.022 | ±0.001 | - | ✓ |
| subshell_isolation | bash | 6.233 | ±6.070 | - | ✓ |
| subshell_nested | bashkit | 0.027 | ±0.001 | - | ✓ |
| subshell_nested | bash | 8.139 | ±3.945 | - | ✓ |
| subshell_pipeline | bashkit | 0.047 | ±0.004 | - | ✓ |
| subshell_pipeline | bash | 51.381 | ±17.176 | - | ✓ |
| subshell_capture_loop | bashkit | 0.045 | ±0.002 | - | ✓ |
| subshell_capture_loop | bash | 11.158 | ±2.938 | - | ✓ |
| subshell_process_subst | bashkit | 0.029 | ±0.001 | - | ✓ |
| subshell_process_subst | bash | 7.270 | ±1.816 | - | ✓ |

### Tools

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| tool_grep_simple | bashkit | 0.022 | ±0.002 | - | ✓ |
| tool_grep_simple | bash | 13.882 | ±6.248 | - | ✓ |
| tool_grep_case | bashkit | 1.787 | ±2.795 | - | ✓ |
| tool_grep_case | bash | 13.085 | ±6.850 | - | ✓ |
| tool_grep_count | bashkit | 0.023 | ±0.002 | - | ✓ |
| tool_grep_count | bash | 11.065 | ±4.521 | - | ✓ |
| tool_grep_invert | bashkit | 0.068 | ±0.083 | - | ✓ |
| tool_grep_invert | bash | 14.331 | ±5.223 | - | ✓ |
| tool_grep_regex | bashkit | 0.066 | ±0.035 | - | ✓ |
| tool_grep_regex | bash | 14.363 | ±3.163 | - | ✓ |
| tool_sed_replace | bashkit | 0.057 | ±0.003 | - | ✓ |
| tool_sed_replace | bash | 9.419 | ±2.948 | - | ✓ |
| tool_sed_global | bashkit | 0.057 | ±0.003 | - | ✓ |
| tool_sed_global | bash | 9.460 | ±2.834 | - | ✓ |
| tool_sed_delete | bashkit | 0.020 | ±0.001 | - | ✓ |
| tool_sed_delete | bash | 10.257 | ±4.791 | - | ✓ |
| tool_sed_lines | bashkit | 0.022 | ±0.007 | - | ✓ |
| tool_sed_lines | bash | 9.648 | ±5.290 | - | ✓ |
| tool_sed_backrefs | bashkit | 0.078 | ±0.007 | - | ✓ |
| tool_sed_backrefs | bash | 10.159 | ±4.331 | - | ✓ |
| tool_awk_print | bashkit | 0.021 | ±0.002 | - | ✓ |
| tool_awk_print | bash | 8.059 | ±2.451 | - | ✓ |
| tool_awk_sum | bashkit | 0.023 | ±0.001 | - | ✓ |
| tool_awk_sum | bash | 9.403 | ±4.043 | - | ✓ |
| tool_awk_pattern | bashkit | 0.031 | ±0.002 | - | ✓ |
| tool_awk_pattern | bash | 10.155 | ±2.492 | - | ✓ |
| tool_awk_fieldsep | bashkit | 0.020 | ±0.001 | - | ✓ |
| tool_awk_fieldsep | bash | 10.116 | ±4.357 | - | ✓ |
| tool_awk_nf | bashkit | 0.020 | ±0.001 | - | ✓ |
| tool_awk_nf | bash | 12.800 | ±5.745 | - | ✓ |
| tool_awk_compute | bashkit | 0.020 | ±0.001 | - | ✓ |
| tool_awk_compute | bash | 10.985 | ±2.700 | - | ✓ |
| tool_jq_identity | bashkit | 0.249 | ±0.014 | - | ✓ |
| tool_jq_identity | bash | 11.116 | ±4.503 | - | ✓ |
| tool_jq_field | bashkit | 0.244 | ±0.013 | - | ✓ |
| tool_jq_field | bash | 9.068 | ±2.884 | - | ✓ |
| tool_jq_array | bashkit | 0.650 | ±0.263 | - | ✓ |
| tool_jq_array | bash | 12.901 | ±5.862 | - | ✓ |
| tool_jq_filter | bashkit | 0.250 | ±0.018 | - | ✓ |
| tool_jq_filter | bash | 10.880 | ±3.260 | - | ✓ |
| tool_jq_map | bashkit | 0.382 | ±0.240 | - | ✓ |
| tool_jq_map | bash | 10.667 | ±3.244 | - | ✓ |

### Variables

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| var_assign_simple | bashkit | 0.064 | ±0.139 | - | ✓ |
| var_assign_simple | bash | 10.405 | ±6.939 | - | ✓ |
| var_assign_many | bashkit | 1.554 | ±3.485 | - | ✓ |
| var_assign_many | bash | 7.201 | ±4.621 | - | ✓ |
| var_default | bashkit | 0.302 | ±0.786 | - | ✓ |
| var_default | bash | 5.340 | ±1.782 | - | ✓ |
| var_length | bashkit | 0.017 | ±0.001 | - | ✓ |
| var_length | bash | 7.034 | ±4.333 | - | ✓ |
| var_substring | bashkit | 0.018 | ±0.001 | - | ✓ |
| var_substring | bash | 7.026 | ±2.972 | - | ✓ |
| var_replace | bashkit | 0.063 | ±0.046 | - | ✓ |
| var_replace | bash | 7.640 | ±5.310 | - | ✓ |
| var_nested | bashkit | 0.019 | ±0.001 | - | ✓ |
| var_nested | bash | 7.555 | ±3.479 | - | ✓ |
| var_export | bashkit | 0.159 | ±0.315 | - | ✓ |
| var_export | bash | 6.428 | ±2.431 | - | ✓ |

## Runner Descriptions

| Runner | Type | Description |
|--------|------|-------------|
| bashkit | in-process | Rust library call, no fork/exec |
| bashkit-cli | subprocess | bashkit binary, new process per run |
| bashkit-js | persistent child | Node.js + @everruns/bashkit, warm interpreter |
| bashkit-py | persistent child | Python + bashkit package, warm interpreter |
| bash | subprocess | /bin/bash, new process per run |
| gbash | subprocess | gbash binary (Go), new process per run |
| gbash-server | persistent child | gbash JSON-RPC server, warm interpreter |
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
