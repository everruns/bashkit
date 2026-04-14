# Bashkit Benchmark Report

## System Information

- **Moniker**: `runsc-linux-x86_64`
- **Hostname**: runsc
- **OS**: linux
- **Architecture**: x86_64
- **CPUs**: 16
- **Timestamp**: 1776121540
- **Iterations**: 10
- **Warmup**: 2
- **Prewarm cases**: 3

## Summary

Benchmarked 96 cases across 2 runners.

| Runner | Total Time (ms) | Avg/Case (ms) | Errors | Error Rate | Output Match |
|--------|-----------------|---------------|--------|------------|-------------|
| bashkit | 41.52 | 0.433 | 0 | 0.0% | 100.0% |
| bash | 4449.32 | 46.347 | 0 | 0.0% | 100.0% |

## Performance Comparison

**Bashkit is 107.2x faster** than bash on average.

## Results by Category

### Arithmetic

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arith_basic | bashkit | 0.102 | ±0.043 | - | ✓ |
| arith_basic | bash | 8.855 | ±0.541 | - | ✓ |
| arith_complex | bashkit | 0.091 | ±0.022 | - | ✓ |
| arith_complex | bash | 7.814 | ±0.213 | - | ✓ |
| arith_variables | bashkit | 0.074 | ±0.010 | - | ✓ |
| arith_variables | bash | 8.379 | ±0.677 | - | ✓ |
| arith_increment | bashkit | 0.090 | ±0.027 | - | ✓ |
| arith_increment | bash | 8.152 | ±0.272 | - | ✓ |
| arith_modulo | bashkit | 0.064 | ±0.020 | - | ✓ |
| arith_modulo | bash | 7.770 | ±0.335 | - | ✓ |
| arith_loop_sum | bashkit | 0.168 | ±0.066 | - | ✓ |
| arith_loop_sum | bash | 8.086 | ±0.320 | - | ✓ |

### Arrays

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arr_create | bashkit | 0.087 | ±0.026 | - | ✓ |
| arr_create | bash | 7.943 | ±0.399 | - | ✓ |
| arr_all | bashkit | 0.114 | ±0.028 | - | ✓ |
| arr_all | bash | 7.817 | ±0.309 | - | ✓ |
| arr_length | bashkit | 0.112 | ±0.068 | - | ✓ |
| arr_length | bash | 8.087 | ±0.309 | - | ✓ |
| arr_iterate | bashkit | 0.093 | ±0.026 | - | ✓ |
| arr_iterate | bash | 8.514 | ±0.558 | - | ✓ |
| arr_slice | bashkit | 0.094 | ±0.028 | - | ✓ |
| arr_slice | bash | 8.399 | ±0.392 | - | ✓ |
| arr_assign_index | bashkit | 0.091 | ±0.040 | - | ✓ |
| arr_assign_index | bash | 8.132 | ±0.278 | - | ✓ |

### Complex

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| complex_fibonacci | bashkit | 4.399 | ±0.305 | - | ✓ |
| complex_fibonacci | bash | 524.777 | ±12.833 | - | ✓ |
| complex_fibonacci_iter | bashkit | 0.148 | ±0.018 | - | ✓ |
| complex_fibonacci_iter | bash | 8.100 | ±0.290 | - | ✓ |
| complex_nested_subst | bashkit | 0.098 | ±0.035 | - | ✓ |
| complex_nested_subst | bash | 17.530 | ±1.660 | - | ✓ |
| complex_loop_compute | bashkit | 0.185 | ±0.051 | - | ✓ |
| complex_loop_compute | bash | 8.650 | ±0.926 | - | ✓ |
| complex_string_build | bashkit | 0.114 | ±0.033 | - | ✓ |
| complex_string_build | bash | 8.450 | ±0.899 | - | ✓ |
| complex_json_transform | bashkit | 0.675 | ±0.075 | - | ✓ |
| complex_json_transform | bash | 24.916 | ±1.466 | - | ✓ |
| complex_pipeline_text | bashkit | 0.332 | ±0.070 | - | ✓ |
| complex_pipeline_text | bash | 23.988 | ±0.717 | - | ✓ |

### Control

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| ctrl_if_simple | bashkit | 0.092 | ±0.034 | - | ✓ |
| ctrl_if_simple | bash | 8.449 | ±0.622 | - | ✓ |
| ctrl_if_else | bashkit | 0.094 | ±0.037 | - | ✓ |
| ctrl_if_else | bash | 7.735 | ±0.301 | - | ✓ |
| ctrl_for_list | bashkit | 0.078 | ±0.015 | - | ✓ |
| ctrl_for_list | bash | 7.919 | ±0.415 | - | ✓ |
| ctrl_for_range | bashkit | 0.095 | ±0.013 | - | ✓ |
| ctrl_for_range | bash | 8.272 | ±0.701 | - | ✓ |
| ctrl_while | bashkit | 0.121 | ±0.014 | - | ✓ |
| ctrl_while | bash | 8.109 | ±0.224 | - | ✓ |
| ctrl_case | bashkit | 0.112 | ±0.029 | - | ✓ |
| ctrl_case | bash | 12.450 | ±13.139 | - | ✓ |
| ctrl_function | bashkit | 0.089 | ±0.036 | - | ✓ |
| ctrl_function | bash | 7.964 | ±0.296 | - | ✓ |
| ctrl_function_return | bashkit | 0.090 | ±0.016 | - | ✓ |
| ctrl_function_return | bash | 11.033 | ±0.584 | - | ✓ |
| ctrl_nested_loops | bashkit | 0.103 | ±0.008 | - | ✓ |
| ctrl_nested_loops | bash | 8.163 | ±0.417 | - | ✓ |

### Io

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| io_redirect_write | bashkit | 0.075 | ±0.009 | - | ✓ |
| io_redirect_write | bash | 24.110 | ±0.911 | - | ✓ |
| io_append | bashkit | 0.111 | ±0.037 | - | ✓ |
| io_append | bash | 24.544 | ±0.744 | - | ✓ |
| io_dev_null | bashkit | 0.085 | ±0.033 | - | ✓ |
| io_dev_null | bash | 8.195 | ±0.401 | - | ✓ |
| io_stderr_redirect | bashkit | 0.082 | ±0.033 | - | ✓ |
| io_stderr_redirect | bash | 8.386 | ±0.493 | - | ✓ |
| io_read_lines | bashkit | 0.113 | ±0.010 | - | ✓ |
| io_read_lines | bash | 8.696 | ±0.255 | - | ✓ |
| io_multiline_heredoc | bashkit | 0.090 | ±0.031 | - | ✓ |
| io_multiline_heredoc | bash | 20.599 | ±0.860 | - | ✓ |

### Large

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| large_loop_1000 | bashkit | 4.298 | ±0.094 | - | ✓ |
| large_loop_1000 | bash | 11.056 | ±0.652 | - | ✓ |
| large_string_append_100 | bashkit | 0.429 | ±0.047 | - | ✓ |
| large_string_append_100 | bash | 8.470 | ±0.604 | - | ✓ |
| large_array_fill_200 | bashkit | 0.790 | ±0.081 | - | ✓ |
| large_array_fill_200 | bash | 8.719 | ±0.428 | - | ✓ |
| large_nested_loops | bashkit | 2.084 | ±0.122 | - | ✓ |
| large_nested_loops | bash | 9.895 | ±0.945 | - | ✓ |
| large_fibonacci_12 | bashkit | 10.643 | ±0.567 | - | ✓ |
| large_fibonacci_12 | bash | 1415.775 | ±23.356 | - | ✓ |
| large_function_calls_500 | bashkit | 5.003 | ±0.277 | - | ✓ |
| large_function_calls_500 | bash | 1232.167 | ±63.476 | - | ✓ |
| large_multiline_script | bashkit | 0.523 | ±0.075 | - | ✓ |
| large_multiline_script | bash | 9.088 | ±1.490 | - | ✓ |
| large_pipeline_chain | bashkit | 0.840 | ±0.031 | - | ✓ |
| large_pipeline_chain | bash | 28.027 | ±1.282 | - | ✓ |
| large_assoc_array | bashkit | 0.118 | ±0.027 | - | ✓ |
| large_assoc_array | bash | 8.975 | ±0.690 | - | ✓ |

### Pipes

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| pipe_simple | bashkit | 0.087 | ±0.021 | - | ✓ |
| pipe_simple | bash | 19.123 | ±1.097 | - | ✓ |
| pipe_multi | bashkit | 0.079 | ±0.024 | - | ✓ |
| pipe_multi | bash | 23.610 | ±0.727 | - | ✓ |
| pipe_command_subst | bashkit | 0.106 | ±0.036 | - | ✓ |
| pipe_command_subst | bash | 11.008 | ±0.605 | - | ✓ |
| pipe_heredoc | bashkit | 0.076 | ±0.026 | - | ✓ |
| pipe_heredoc | bash | 17.154 | ±0.620 | - | ✓ |
| pipe_herestring | bashkit | 0.077 | ±0.034 | - | ✓ |
| pipe_herestring | bash | 17.158 | ±0.609 | - | ✓ |
| pipe_discard | bashkit | 0.103 | ±0.029 | - | ✓ |
| pipe_discard | bash | 11.398 | ±0.534 | - | ✓ |

### Startup

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| startup_empty | bashkit | 0.070 | ±0.023 | - | ✓ |
| startup_empty | bash | 8.445 | ±0.553 | - | ✓ |
| startup_true | bashkit | 0.070 | ±0.021 | - | ✓ |
| startup_true | bash | 8.302 | ±0.476 | - | ✓ |
| startup_echo | bashkit | 0.071 | ±0.018 | - | ✓ |
| startup_echo | bash | 8.097 | ±0.676 | - | ✓ |
| startup_exit | bashkit | 0.074 | ±0.022 | - | ✓ |
| startup_exit | bash | 8.174 | ±0.612 | - | ✓ |

### Strings

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| str_concat | bashkit | 0.098 | ±0.024 | - | ✓ |
| str_concat | bash | 8.234 | ±0.232 | - | ✓ |
| str_printf | bashkit | 0.095 | ±0.021 | - | ✓ |
| str_printf | bash | 8.157 | ±0.677 | - | ✓ |
| str_printf_pad | bashkit | 0.071 | ±0.018 | - | ✓ |
| str_printf_pad | bash | 8.181 | ±0.506 | - | ✓ |
| str_echo_escape | bashkit | 0.067 | ±0.027 | - | ✓ |
| str_echo_escape | bash | 8.167 | ±0.322 | - | ✓ |
| str_prefix_strip | bashkit | 0.097 | ±0.029 | - | ✓ |
| str_prefix_strip | bash | 8.327 | ±0.357 | - | ✓ |
| str_suffix_strip | bashkit | 0.086 | ±0.025 | - | ✓ |
| str_suffix_strip | bash | 8.581 | ±0.716 | - | ✓ |
| str_uppercase | bashkit | 0.059 | ±0.008 | - | ✓ |
| str_uppercase | bash | 8.428 | ±0.472 | - | ✓ |
| str_lowercase | bashkit | 0.084 | ±0.029 | - | ✓ |
| str_lowercase | bash | 8.279 | ±0.646 | - | ✓ |

### Subshell

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| subshell_simple | bashkit | 0.082 | ±0.035 | - | ✓ |
| subshell_simple | bash | 11.126 | ±0.511 | - | ✓ |
| subshell_isolation | bashkit | 0.094 | ±0.037 | - | ✓ |
| subshell_isolation | bash | 11.719 | ±0.832 | - | ✓ |
| subshell_nested | bashkit | 0.106 | ±0.016 | - | ✓ |
| subshell_nested | bash | 19.611 | ±0.600 | - | ✓ |
| subshell_pipeline | bashkit | 0.078 | ±0.029 | - | ✓ |
| subshell_pipeline | bash | 21.330 | ±1.439 | - | ✓ |
| subshell_capture_loop | bashkit | 0.164 | ±0.034 | - | ✓ |
| subshell_capture_loop | bash | 21.413 | ±0.661 | - | ✓ |
| subshell_process_subst | bashkit | 0.116 | ±0.033 | - | ✓ |
| subshell_process_subst | bash | 13.944 | ±0.890 | - | ✓ |

### Tools

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| tool_grep_simple | bashkit | 0.143 | ±0.048 | - | ✓ |
| tool_grep_simple | bash | 22.445 | ±2.085 | - | ✓ |
| tool_grep_case | bashkit | 0.237 | ±0.068 | - | ✓ |
| tool_grep_case | bash | 24.278 | ±7.311 | - | ✓ |
| tool_grep_count | bashkit | 0.079 | ±0.027 | - | ✓ |
| tool_grep_count | bash | 20.436 | ±0.994 | - | ✓ |
| tool_grep_invert | bashkit | 0.094 | ±0.030 | - | ✓ |
| tool_grep_invert | bash | 20.478 | ±1.406 | - | ✓ |
| tool_grep_regex | bashkit | 0.100 | ±0.011 | - | ✓ |
| tool_grep_regex | bash | 20.554 | ±0.919 | - | ✓ |
| tool_sed_replace | bashkit | 0.199 | ±0.017 | - | ✓ |
| tool_sed_replace | bash | 21.887 | ±0.618 | - | ✓ |
| tool_sed_global | bashkit | 0.177 | ±0.019 | - | ✓ |
| tool_sed_global | bash | 22.133 | ±0.993 | - | ✓ |
| tool_sed_delete | bashkit | 0.093 | ±0.030 | - | ✓ |
| tool_sed_delete | bash | 21.511 | ±0.846 | - | ✓ |
| tool_sed_lines | bashkit | 0.112 | ±0.037 | - | ✓ |
| tool_sed_lines | bash | 21.723 | ±0.790 | - | ✓ |
| tool_sed_backrefs | bashkit | 0.258 | ±0.027 | - | ✓ |
| tool_sed_backrefs | bash | 21.868 | ±1.478 | - | ✓ |
| tool_awk_print | bashkit | 0.076 | ±0.017 | - | ✓ |
| tool_awk_print | bash | 20.244 | ±0.878 | - | ✓ |
| tool_awk_sum | bashkit | 0.117 | ±0.045 | - | ✓ |
| tool_awk_sum | bash | 20.203 | ±0.663 | - | ✓ |
| tool_awk_pattern | bashkit | 0.118 | ±0.045 | - | ✓ |
| tool_awk_pattern | bash | 19.808 | ±0.924 | - | ✓ |
| tool_awk_fieldsep | bashkit | 0.082 | ±0.027 | - | ✓ |
| tool_awk_fieldsep | bash | 19.874 | ±0.739 | - | ✓ |
| tool_awk_nf | bashkit | 0.084 | ±0.028 | - | ✓ |
| tool_awk_nf | bash | 19.460 | ±0.285 | - | ✓ |
| tool_awk_compute | bashkit | 0.078 | ±0.026 | - | ✓ |
| tool_awk_compute | bash | 19.841 | ±0.797 | - | ✓ |
| tool_jq_identity | bashkit | 0.673 | ±0.072 | - | ✓ |
| tool_jq_identity | bash | 22.900 | ±0.691 | - | ✓ |
| tool_jq_field | bashkit | 0.630 | ±0.011 | - | ✓ |
| tool_jq_field | bash | 24.294 | ±1.187 | - | ✓ |
| tool_jq_array | bashkit | 0.656 | ±0.015 | - | ✓ |
| tool_jq_array | bash | 25.082 | ±1.447 | - | ✓ |
| tool_jq_filter | bashkit | 0.644 | ±0.010 | - | ✓ |
| tool_jq_filter | bash | 28.546 | ±11.273 | - | ✓ |
| tool_jq_map | bashkit | 0.717 | ±0.098 | - | ✓ |
| tool_jq_map | bash | 26.097 | ±0.908 | - | ✓ |

### Variables

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| var_assign_simple | bashkit | 0.090 | ±0.026 | - | ✓ |
| var_assign_simple | bash | 7.970 | ±0.380 | - | ✓ |
| var_assign_many | bashkit | 0.131 | ±0.061 | - | ✓ |
| var_assign_many | bash | 8.411 | ±0.314 | - | ✓ |
| var_default | bashkit | 0.071 | ±0.029 | - | ✓ |
| var_default | bash | 8.436 | ±0.438 | - | ✓ |
| var_length | bashkit | 0.089 | ±0.023 | - | ✓ |
| var_length | bash | 8.303 | ±0.459 | - | ✓ |
| var_substring | bashkit | 0.087 | ±0.030 | - | ✓ |
| var_substring | bash | 8.071 | ±0.342 | - | ✓ |
| var_replace | bashkit | 0.083 | ±0.024 | - | ✓ |
| var_replace | bash | 8.455 | ±0.392 | - | ✓ |
| var_nested | bashkit | 0.118 | ±0.078 | - | ✓ |
| var_nested | bash | 8.179 | ±0.344 | - | ✓ |
| var_export | bashkit | 0.089 | ±0.023 | - | ✓ |
| var_export | bash | 8.715 | ±0.823 | - | ✓ |

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

