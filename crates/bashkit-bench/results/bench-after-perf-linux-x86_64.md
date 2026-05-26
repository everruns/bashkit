# Bashkit Benchmark Report

## System Information

- **Moniker**: `vm-linux-x86_64`
- **Hostname**: vm
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 4
- **Timestamp**: 1779744905
- **Iterations**: 10
- **Warmup**: 2
- **Prewarm cases**: 3

## Summary

Benchmarked 96 cases across 2 runners.

| Runner | Total Time (ms) | Avg/Case (ms) | Errors | Error Rate | Output Match |
|--------|-----------------|---------------|--------|------------|-------------|
| bashkit | 43.16 | 0.450 | 0 | 0.0% | 100.0% |
| bash | 1095.06 | 11.407 | 0 | 0.0% | 100.0% |

## Performance Comparison

**Bashkit is 25.4x faster** than bash on average.

## Results by Category

### Arithmetic

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arith_basic | bashkit | 0.044 | ±0.007 | - | ✓ |
| arith_basic | bash | 1.680 | ±0.067 | - | ✓ |
| arith_complex | bashkit | 0.060 | ±0.043 | - | ✓ |
| arith_complex | bash | 2.002 | ±0.834 | - | ✓ |
| arith_variables | bashkit | 0.047 | ±0.003 | - | ✓ |
| arith_variables | bash | 2.976 | ±2.490 | - | ✓ |
| arith_increment | bashkit | 0.045 | ±0.004 | - | ✓ |
| arith_increment | bash | 3.438 | ±2.058 | - | ✓ |
| arith_modulo | bashkit | 0.042 | ±0.002 | - | ✓ |
| arith_modulo | bash | 2.844 | ±2.364 | - | ✓ |
| arith_loop_sum | bashkit | 0.081 | ±0.011 | - | ✓ |
| arith_loop_sum | bash | 3.339 | ±2.826 | - | ✓ |

### Arrays

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arr_create | bashkit | 0.045 | ±0.010 | - | ✓ |
| arr_create | bash | 2.008 | ±0.896 | - | ✓ |
| arr_all | bashkit | 0.044 | ±0.008 | - | ✓ |
| arr_all | bash | 2.066 | ±0.883 | - | ✓ |
| arr_length | bashkit | 0.040 | ±0.002 | - | ✓ |
| arr_length | bash | 2.534 | ±2.487 | - | ✓ |
| arr_iterate | bashkit | 0.047 | ±0.002 | - | ✓ |
| arr_iterate | bash | 2.009 | ±0.620 | - | ✓ |
| arr_slice | bashkit | 0.050 | ±0.019 | - | ✓ |
| arr_slice | bash | 2.645 | ±1.185 | - | ✓ |
| arr_assign_index | bashkit | 0.046 | ±0.010 | - | ✓ |
| arr_assign_index | bash | 2.634 | ±1.764 | - | ✓ |

### Complex

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| complex_fibonacci | bashkit | 5.531 | ±2.055 | - | ✓ |
| complex_fibonacci | bash | 123.415 | ±24.433 | - | ✓ |
| complex_fibonacci_iter | bashkit | 0.101 | ±0.018 | - | ✓ |
| complex_fibonacci_iter | bash | 1.827 | ±0.081 | - | ✓ |
| complex_nested_subst | bashkit | 0.059 | ±0.013 | - | ✓ |
| complex_nested_subst | bash | 3.226 | ±0.326 | - | ✓ |
| complex_loop_compute | bashkit | 0.117 | ±0.026 | - | ✓ |
| complex_loop_compute | bash | 2.479 | ±1.460 | - | ✓ |
| complex_string_build | bashkit | 0.078 | ±0.018 | - | ✓ |
| complex_string_build | bash | 2.696 | ±1.899 | - | ✓ |
| complex_json_transform | bashkit | 0.629 | ±0.035 | - | ✓ |
| complex_json_transform | bash | 5.054 | ±0.690 | - | ✓ |
| complex_pipeline_text | bashkit | 0.215 | ±0.023 | - | ✓ |
| complex_pipeline_text | bash | 5.983 | ±3.395 | - | ✓ |

### Control

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| ctrl_if_simple | bashkit | 0.046 | ±0.010 | - | ✓ |
| ctrl_if_simple | bash | 1.722 | ±0.076 | - | ✓ |
| ctrl_if_else | bashkit | 0.049 | ±0.014 | - | ✓ |
| ctrl_if_else | bash | 1.721 | ±0.072 | - | ✓ |
| ctrl_for_list | bashkit | 0.056 | ±0.004 | - | ✓ |
| ctrl_for_list | bash | 2.362 | ±1.289 | - | ✓ |
| ctrl_for_range | bashkit | 0.063 | ±0.011 | - | ✓ |
| ctrl_for_range | bash | 2.884 | ±2.484 | - | ✓ |
| ctrl_while | bashkit | 0.088 | ±0.011 | - | ✓ |
| ctrl_while | bash | 2.444 | ±1.340 | - | ✓ |
| ctrl_case | bashkit | 0.052 | ±0.011 | - | ✓ |
| ctrl_case | bash | 1.911 | ±0.346 | - | ✓ |
| ctrl_function | bashkit | 0.050 | ±0.011 | - | ✓ |
| ctrl_function | bash | 1.846 | ±0.203 | - | ✓ |
| ctrl_function_return | bashkit | 0.064 | ±0.009 | - | ✓ |
| ctrl_function_return | bash | 2.651 | ±1.066 | - | ✓ |
| ctrl_nested_loops | bashkit | 0.082 | ±0.027 | - | ✓ |
| ctrl_nested_loops | bash | 2.553 | ±1.465 | - | ✓ |

### Io

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| io_redirect_write | bashkit | 0.102 | ±0.019 | - | ✓ |
| io_redirect_write | bash | 4.814 | ±1.045 | - | ✓ |
| io_append | bashkit | 0.086 | ±0.013 | - | ✓ |
| io_append | bash | 5.332 | ±1.664 | - | ✓ |
| io_dev_null | bashkit | 0.039 | ±0.003 | - | ✓ |
| io_dev_null | bash | 2.085 | ±1.105 | - | ✓ |
| io_stderr_redirect | bashkit | 0.040 | ±0.005 | - | ✓ |
| io_stderr_redirect | bash | 1.679 | ±0.050 | - | ✓ |
| io_read_lines | bashkit | 0.081 | ±0.011 | - | ✓ |
| io_read_lines | bash | 2.104 | ±0.958 | - | ✓ |
| io_multiline_heredoc | bashkit | 0.062 | ±0.008 | - | ✓ |
| io_multiline_heredoc | bash | 4.359 | ±2.634 | - | ✓ |

### Large

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| large_loop_1000 | bashkit | 5.464 | ±2.379 | - | ✓ |
| large_loop_1000 | bash | 4.956 | ±1.351 | - | ✓ |
| large_string_append_100 | bashkit | 0.267 | ±0.041 | - | ✓ |
| large_string_append_100 | bash | 2.292 | ±0.644 | - | ✓ |
| large_array_fill_200 | bashkit | 1.589 | ±0.068 | - | ✓ |
| large_array_fill_200 | bash | 2.374 | ±0.141 | - | ✓ |
| large_nested_loops | bashkit | 1.554 | ±0.099 | - | ✓ |
| large_nested_loops | bash | 3.005 | ±0.273 | - | ✓ |
| large_fibonacci_12 | bashkit | 13.221 | ±2.147 | - | ✓ |
| large_fibonacci_12 | bash | 413.271 | ±35.841 | - | ✓ |
| large_function_calls_500 | bashkit | 5.399 | ±0.267 | - | ✓ |
| large_function_calls_500 | bash | 236.057 | ±66.219 | - | ✓ |
| large_multiline_script | bashkit | 0.418 | ±0.038 | - | ✓ |
| large_multiline_script | bash | 1.973 | ±0.104 | - | ✓ |
| large_pipeline_chain | bashkit | 0.812 | ±0.085 | - | ✓ |
| large_pipeline_chain | bash | 9.110 | ±3.689 | - | ✓ |
| large_assoc_array | bashkit | 0.050 | ±0.010 | - | ✓ |
| large_assoc_array | bash | 2.607 | ±1.320 | - | ✓ |

### Pipes

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| pipe_simple | bashkit | 0.061 | ±0.012 | - | ✓ |
| pipe_simple | bash | 6.908 | ±3.922 | - | ✓ |
| pipe_multi | bashkit | 0.085 | ±0.010 | - | ✓ |
| pipe_multi | bash | 8.661 | ±4.051 | - | ✓ |
| pipe_command_subst | bashkit | 0.047 | ±0.008 | - | ✓ |
| pipe_command_subst | bash | 2.483 | ±0.834 | - | ✓ |
| pipe_heredoc | bashkit | 0.053 | ±0.003 | - | ✓ |
| pipe_heredoc | bash | 4.205 | ±1.713 | - | ✓ |
| pipe_herestring | bashkit | 0.057 | ±0.010 | - | ✓ |
| pipe_herestring | bash | 4.943 | ±3.246 | - | ✓ |
| pipe_discard | bashkit | 0.044 | ±0.003 | - | ✓ |
| pipe_discard | bash | 3.122 | ±1.840 | - | ✓ |

### Startup

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| startup_empty | bashkit | 0.061 | ±0.010 | - | ✓ |
| startup_empty | bash | 1.596 | ±0.075 | - | ✓ |
| startup_true | bashkit | 0.045 | ±0.011 | - | ✓ |
| startup_true | bash | 2.113 | ±1.133 | - | ✓ |
| startup_echo | bashkit | 0.042 | ±0.011 | - | ✓ |
| startup_echo | bash | 1.710 | ±0.083 | - | ✓ |
| startup_exit | bashkit | 0.040 | ±0.009 | - | ✓ |
| startup_exit | bash | 2.391 | ±1.995 | - | ✓ |

### Strings

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| str_concat | bashkit | 0.045 | ±0.008 | - | ✓ |
| str_concat | bash | 2.539 | ±1.517 | - | ✓ |
| str_printf | bashkit | 0.039 | ±0.003 | - | ✓ |
| str_printf | bash | 1.916 | ±0.686 | - | ✓ |
| str_printf_pad | bashkit | 0.038 | ±0.003 | - | ✓ |
| str_printf_pad | bash | 1.850 | ±0.212 | - | ✓ |
| str_echo_escape | bashkit | 0.041 | ±0.009 | - | ✓ |
| str_echo_escape | bash | 2.508 | ±2.125 | - | ✓ |
| str_prefix_strip | bashkit | 0.044 | ±0.010 | - | ✓ |
| str_prefix_strip | bash | 3.202 | ±2.560 | - | ✓ |
| str_suffix_strip | bashkit | 0.047 | ±0.017 | - | ✓ |
| str_suffix_strip | bash | 2.440 | ±2.183 | - | ✓ |
| str_uppercase | bashkit | 0.044 | ±0.009 | - | ✓ |
| str_uppercase | bash | 2.177 | ±0.599 | - | ✓ |
| str_lowercase | bashkit | 0.044 | ±0.009 | - | ✓ |
| str_lowercase | bash | 2.216 | ±1.217 | - | ✓ |

### Subshell

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| subshell_simple | bashkit | 0.042 | ±0.008 | - | ✓ |
| subshell_simple | bash | 2.621 | ±1.287 | - | ✓ |
| subshell_isolation | bashkit | 0.048 | ±0.010 | - | ✓ |
| subshell_isolation | bash | 2.277 | ±0.073 | - | ✓ |
| subshell_nested | bashkit | 0.052 | ±0.002 | - | ✓ |
| subshell_nested | bash | 3.842 | ±0.150 | - | ✓ |
| subshell_pipeline | bashkit | 0.045 | ±0.010 | - | ✓ |
| subshell_pipeline | bash | 5.799 | ±1.668 | - | ✓ |
| subshell_capture_loop | bashkit | 0.090 | ±0.015 | - | ✓ |
| subshell_capture_loop | bash | 5.338 | ±2.355 | - | ✓ |
| subshell_process_subst | bashkit | 0.063 | ±0.010 | - | ✓ |
| subshell_process_subst | bash | 2.613 | ±0.192 | - | ✓ |

### Tools

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| tool_grep_simple | bashkit | 0.048 | ±0.003 | - | ✓ |
| tool_grep_simple | bash | 5.994 | ±1.680 | - | ✓ |
| tool_grep_case | bashkit | 0.162 | ±0.010 | - | ✓ |
| tool_grep_case | bash | 4.428 | ±1.675 | - | ✓ |
| tool_grep_count | bashkit | 0.057 | ±0.027 | - | ✓ |
| tool_grep_count | bash | 5.227 | ±2.326 | - | ✓ |
| tool_grep_invert | bashkit | 0.054 | ±0.012 | - | ✓ |
| tool_grep_invert | bash | 5.682 | ±3.059 | - | ✓ |
| tool_grep_regex | bashkit | 0.075 | ±0.010 | - | ✓ |
| tool_grep_regex | bash | 3.745 | ±1.055 | - | ✓ |
| tool_sed_replace | bashkit | 0.132 | ±0.012 | - | ✓ |
| tool_sed_replace | bash | 4.576 | ±1.407 | - | ✓ |
| tool_sed_global | bashkit | 0.140 | ±0.032 | - | ✓ |
| tool_sed_global | bash | 4.540 | ±1.689 | - | ✓ |
| tool_sed_delete | bashkit | 0.063 | ±0.011 | - | ✓ |
| tool_sed_delete | bash | 5.494 | ±1.692 | - | ✓ |
| tool_sed_lines | bashkit | 0.043 | ±0.005 | - | ✓ |
| tool_sed_lines | bash | 4.791 | ±2.574 | - | ✓ |
| tool_sed_backrefs | bashkit | 0.257 | ±0.058 | - | ✓ |
| tool_sed_backrefs | bash | 4.889 | ±2.530 | - | ✓ |
| tool_awk_print | bashkit | 0.048 | ±0.011 | - | ✓ |
| tool_awk_print | bash | 5.480 | ±2.587 | - | ✓ |
| tool_awk_sum | bashkit | 0.053 | ±0.010 | - | ✓ |
| tool_awk_sum | bash | 5.143 | ±2.859 | - | ✓ |
| tool_awk_pattern | bashkit | 0.085 | ±0.018 | - | ✓ |
| tool_awk_pattern | bash | 5.325 | ±3.695 | - | ✓ |
| tool_awk_fieldsep | bashkit | 0.048 | ±0.005 | - | ✓ |
| tool_awk_fieldsep | bash | 3.331 | ±0.067 | - | ✓ |
| tool_awk_nf | bashkit | 0.046 | ±0.004 | - | ✓ |
| tool_awk_nf | bash | 3.964 | ±1.188 | - | ✓ |
| tool_awk_compute | bashkit | 0.045 | ±0.002 | - | ✓ |
| tool_awk_compute | bash | 3.603 | ±0.252 | - | ✓ |
| tool_jq_identity | bashkit | 0.656 | ±0.090 | - | ✓ |
| tool_jq_identity | bash | 5.965 | ±1.753 | - | ✓ |
| tool_jq_field | bashkit | 0.621 | ±0.030 | - | ✓ |
| tool_jq_field | bash | 5.078 | ±0.902 | - | ✓ |
| tool_jq_array | bashkit | 0.609 | ±0.026 | - | ✓ |
| tool_jq_array | bash | 7.987 | ±3.743 | - | ✓ |
| tool_jq_filter | bashkit | 0.633 | ±0.042 | - | ✓ |
| tool_jq_filter | bash | 5.453 | ±1.247 | - | ✓ |
| tool_jq_map | bashkit | 0.619 | ±0.028 | - | ✓ |
| tool_jq_map | bash | 5.674 | ±1.416 | - | ✓ |

### Variables

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| var_assign_simple | bashkit | 0.047 | ±0.011 | - | ✓ |
| var_assign_simple | bash | 2.340 | ±1.237 | - | ✓ |
| var_assign_many | bashkit | 0.061 | ±0.008 | - | ✓ |
| var_assign_many | bash | 1.744 | ±0.066 | - | ✓ |
| var_default | bashkit | 0.041 | ±0.002 | - | ✓ |
| var_default | bash | 1.609 | ±0.062 | - | ✓ |
| var_length | bashkit | 0.046 | ±0.006 | - | ✓ |
| var_length | bash | 2.295 | ±1.446 | - | ✓ |
| var_substring | bashkit | 0.054 | ±0.018 | - | ✓ |
| var_substring | bash | 3.279 | ±1.877 | - | ✓ |
| var_replace | bashkit | 0.046 | ±0.008 | - | ✓ |
| var_replace | bash | 2.611 | ±1.442 | - | ✓ |
| var_nested | bashkit | 0.053 | ±0.024 | - | ✓ |
| var_nested | bash | 2.660 | ±1.950 | - | ✓ |
| var_export | bashkit | 0.050 | ±0.010 | - | ✓ |
| var_export | bash | 1.716 | ±0.124 | - | ✓ |

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

