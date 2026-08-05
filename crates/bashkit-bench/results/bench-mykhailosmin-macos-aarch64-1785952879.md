# Bashkit Benchmark Report

## System Information

- **Moniker**: `mykhailosmin-macos-aarch64`
- **Hostname**: Mykhailos-Mini.lan
- **OS**: macos
- **Architecture**: aarch64
- **CPUs**: 12
- **Timestamp**: 1785952879
- **Iterations**: 10
- **Warmup**: 2
- **Prewarm cases**: 3

## Summary

Benchmarked 96 cases across 2 runners.

| Runner | Total Time (ms) | Avg/Case (ms) | Errors | Error Rate | Output Match |
|--------|-----------------|---------------|--------|------------|-------------|
| bashkit | 27.97 | 0.291 | 0 | 0.0% | 95.8% |
| bash | 1863.85 | 19.415 | 20 | 2.1% | 100.0% |

## Performance Comparison

**Bashkit is 66.6x faster** than bash on average.

## Results by Category

### Arithmetic

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arith_basic | bashkit | 0.017 | ±0.001 | - | ✓ |
| arith_basic | bash | 6.135 | ±1.978 | - | ✓ |
| arith_complex | bashkit | 0.018 | ±0.002 | - | ✓ |
| arith_complex | bash | 4.491 | ±1.441 | - | ✓ |
| arith_variables | bashkit | 0.020 | ±0.001 | - | ✓ |
| arith_variables | bash | 7.611 | ±5.768 | - | ✓ |
| arith_increment | bashkit | 0.052 | ±0.010 | - | ✓ |
| arith_increment | bash | 5.441 | ±1.519 | - | ✓ |
| arith_modulo | bashkit | 0.026 | ±0.019 | - | ✓ |
| arith_modulo | bash | 4.287 | ±0.954 | - | ✓ |
| arith_loop_sum | bashkit | 0.101 | ±0.068 | - | ✓ |
| arith_loop_sum | bash | 4.984 | ±1.320 | - | ✓ |

### Arrays

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arr_create | bashkit | 0.018 | ±0.001 | - | ✓ |
| arr_create | bash | 3.221 | ±0.755 | - | ✓ |
| arr_all | bashkit | 0.052 | ±0.012 | - | ✓ |
| arr_all | bash | 3.994 | ±1.186 | - | ✓ |
| arr_length | bashkit | 0.057 | ±0.070 | - | ✓ |
| arr_length | bash | 3.974 | ±0.681 | - | ✓ |
| arr_iterate | bashkit | 0.052 | ±0.001 | - | ✓ |
| arr_iterate | bash | 5.505 | ±1.628 | - | ✓ |
| arr_slice | bashkit | 0.019 | ±0.001 | - | ✓ |
| arr_slice | bash | 6.144 | ±1.217 | - | ✓ |
| arr_assign_index | bashkit | 0.045 | ±0.001 | - | ✓ |
| arr_assign_index | bash | 3.118 | ±0.752 | - | ✓ |

### Complex

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| complex_fibonacci | bashkit | 2.029 | ±0.019 | - | ✓ |
| complex_fibonacci | bash | 199.183 | ±29.114 | - | ✓ |
| complex_fibonacci_iter | bashkit | 0.047 | ±0.002 | - | ✓ |
| complex_fibonacci_iter | bash | 4.125 | ±1.112 | - | ✓ |
| complex_nested_subst | bashkit | 0.024 | ±0.001 | - | ✓ |
| complex_nested_subst | bash | 7.628 | ±1.229 | - | ✓ |
| complex_loop_compute | bashkit | 0.049 | ±0.001 | - | ✓ |
| complex_loop_compute | bash | 3.748 | ±0.898 | - | ✓ |
| complex_string_build | bashkit | 0.080 | ±0.070 | - | ✓ |
| complex_string_build | bash | 3.533 | ±1.169 | - | ✓ |
| complex_json_transform | bashkit | 0.274 | ±0.032 | - | ✓ |
| complex_json_transform | bash | 8.970 | ±1.640 | - | ✓ |
| complex_pipeline_text | bashkit | 0.080 | ±0.020 | - | ✓ |
| complex_pipeline_text | bash | 8.335 | ±1.361 | - | ✓ |

### Control

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| ctrl_if_simple | bashkit | 0.018 | ±0.001 | - | ✓ |
| ctrl_if_simple | bash | 6.821 | ±5.116 | - | ✓ |
| ctrl_if_else | bashkit | 0.019 | ±0.001 | - | ✓ |
| ctrl_if_else | bash | 3.824 | ±1.020 | - | ✓ |
| ctrl_for_list | bashkit | 0.068 | ±0.005 | - | ✓ |
| ctrl_for_list | bash | 3.723 | ±0.859 | - | ✓ |
| ctrl_for_range | bashkit | 0.042 | ±0.036 | - | ✓ |
| ctrl_for_range | bash | 5.152 | ±2.254 | - | ✓ |
| ctrl_while | bashkit | 0.112 | ±0.009 | - | ✓ |
| ctrl_while | bash | 4.298 | ±1.178 | - | ✓ |
| ctrl_case | bashkit | 0.020 | ±0.001 | - | ✓ |
| ctrl_case | bash | 5.517 | ±2.827 | - | ✓ |
| ctrl_function | bashkit | 0.021 | ±0.002 | - | ✓ |
| ctrl_function | bash | 13.142 | ±9.755 | - | ✓ |
| ctrl_function_return | bashkit | 0.089 | ±0.054 | - | ✓ |
| ctrl_function_return | bash | 6.500 | ±2.961 | - | ✓ |
| ctrl_nested_loops | bashkit | 0.064 | ±0.027 | - | ✓ |
| ctrl_nested_loops | bash | 5.255 | ±1.798 | - | ✓ |

### Io

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| io_redirect_write | bashkit | 0.042 | ±0.022 | - | ✓ |
| io_redirect_write | bash | 11.763 | ±2.667 | - | ✓ |
| io_append | bashkit | 0.099 | ±0.036 | - | ✓ |
| io_append | bash | 9.317 | ±2.332 | - | ✓ |
| io_dev_null | bashkit | 0.018 | ±0.001 | - | ✓ |
| io_dev_null | bash | 3.474 | ±0.529 | - | ✓ |
| io_stderr_redirect | bashkit | 0.043 | ±0.002 | - | ✓ |
| io_stderr_redirect | bash | 2.058 | ±0.319 | - | ✓ |
| io_read_lines | bashkit | 0.046 | ±0.017 | - | ✓ |
| io_read_lines | bash | 2.183 | ±0.269 | - | ✓ |
| io_multiline_heredoc | bashkit | 0.025 | ±0.002 | - | ✓ |
| io_multiline_heredoc | bash | 5.317 | ±1.196 | - | ✓ |

### Large

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| large_loop_1000 | bashkit | 2.596 | ±0.541 | - | ✓ |
| large_loop_1000 | bash | 6.940 | ±2.485 | - | ✓ |
| large_string_append_100 | bashkit | 1.007 | ±1.934 | - | ✓ |
| large_string_append_100 | bash | 3.873 | ±0.986 | - | ✓ |
| large_array_fill_200 | bashkit | 0.735 | ±0.002 | - | ✓ |
| large_array_fill_200 | bash | 5.630 | ±1.708 | - | ✓ |
| large_nested_loops | bashkit | 1.918 | ±1.573 | - | ✓ |
| large_nested_loops | bash | 8.509 | ±2.038 | - | ✓ |
| large_fibonacci_12 | bashkit | 11.164 | ±9.562 | - | ✓ |
| large_fibonacci_12 | bash | 576.781 | ±42.841 | - | ✓ |
| large_function_calls_500 | bashkit | 2.823 | ±0.012 | - | ✓ |
| large_function_calls_500 | bash | 467.851 | ±136.564 | - | ✓ |
| large_multiline_script | bashkit | 0.204 | ±0.015 | - | ✗ |
| large_multiline_script | bash | 4.293 | ±0.681 | - | ✓ |
| large_pipeline_chain | bashkit | 0.465 | ±0.074 | - | ✓ |
| large_pipeline_chain | bash | 8.169 | ±2.207 | - | ✓ |
| large_assoc_array | bashkit | 0.023 | ±0.001 | - | ✗ |
| large_assoc_array | bash | 1.866 | ±0.289 | - | ✓ |

### Pipes

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| pipe_simple | bashkit | 0.023 | ±0.001 | - | ✓ |
| pipe_simple | bash | 9.171 | ±4.498 | - | ✓ |
| pipe_multi | bashkit | 0.034 | ±0.001 | - | ✓ |
| pipe_multi | bash | 12.932 | ±6.421 | - | ✓ |
| pipe_command_subst | bashkit | 0.045 | ±0.052 | - | ✓ |
| pipe_command_subst | bash | 7.189 | ±3.903 | - | ✓ |
| pipe_heredoc | bashkit | 0.021 | ±0.001 | - | ✓ |
| pipe_heredoc | bash | 9.657 | ±3.830 | - | ✓ |
| pipe_herestring | bashkit | 0.025 | ±0.008 | - | ✓ |
| pipe_herestring | bash | 16.533 | ±6.434 | - | ✓ |
| pipe_discard | bashkit | 0.046 | ±0.001 | - | ✓ |
| pipe_discard | bash | 5.160 | ±1.014 | - | ✓ |

### Startup

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| startup_empty | bashkit | 0.037 | ±0.001 | - | ✓ |
| startup_empty | bash | 4.976 | ±1.644 | - | ✓ |
| startup_true | bashkit | 0.016 | ±0.001 | - | ✓ |
| startup_true | bash | 4.364 | ±1.317 | - | ✓ |
| startup_echo | bashkit | 0.016 | ±0.001 | - | ✓ |
| startup_echo | bash | 4.458 | ±2.441 | - | ✓ |
| startup_exit | bashkit | 0.038 | ±0.002 | - | ✓ |
| startup_exit | bash | 3.817 | ±1.269 | - | ✓ |

### Strings

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| str_concat | bashkit | 0.046 | ±0.026 | - | ✓ |
| str_concat | bash | 8.335 | ±9.530 | - | ✓ |
| str_printf | bashkit | 0.017 | ±0.001 | - | ✓ |
| str_printf | bash | 10.616 | ±8.786 | - | ✓ |
| str_printf_pad | bashkit | 0.016 | ±0.001 | - | ✓ |
| str_printf_pad | bash | 11.921 | ±8.544 | - | ✓ |
| str_echo_escape | bashkit | 0.016 | ±0.001 | - | ✓ |
| str_echo_escape | bash | 13.999 | ±12.888 | - | ✓ |
| str_prefix_strip | bashkit | 0.047 | ±0.008 | - | ✓ |
| str_prefix_strip | bash | 6.385 | ±3.310 | - | ✓ |
| str_suffix_strip | bashkit | 0.018 | ±0.001 | - | ✓ |
| str_suffix_strip | bash | 5.868 | ±1.749 | - | ✓ |
| str_uppercase | bashkit | 0.053 | ±0.036 | - | ✗ |
| str_uppercase | bash | 5.887 | ±2.827 | 10 | ✓ |
| str_lowercase | bashkit | 0.017 | ±0.001 | - | ✗ |
| str_lowercase | bash | 3.590 | ±1.501 | 10 | ✓ |

### Subshell

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| subshell_simple | bashkit | 0.017 | ±0.001 | - | ✓ |
| subshell_simple | bash | 3.186 | ±0.702 | - | ✓ |
| subshell_isolation | bashkit | 0.066 | ±0.019 | - | ✓ |
| subshell_isolation | bash | 4.122 | ±1.129 | - | ✓ |
| subshell_nested | bashkit | 0.027 | ±0.002 | - | ✓ |
| subshell_nested | bash | 6.584 | ±2.148 | - | ✓ |
| subshell_pipeline | bashkit | 0.019 | ±0.001 | - | ✓ |
| subshell_pipeline | bash | 6.619 | ±1.945 | - | ✓ |
| subshell_capture_loop | bashkit | 0.067 | ±0.049 | - | ✓ |
| subshell_capture_loop | bash | 5.829 | ±0.882 | - | ✓ |
| subshell_process_subst | bashkit | 0.072 | ±0.008 | - | ✓ |
| subshell_process_subst | bash | 4.168 | ±0.977 | - | ✓ |

### Tools

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| tool_grep_simple | bashkit | 0.022 | ±0.003 | - | ✓ |
| tool_grep_simple | bash | 11.604 | ±4.262 | - | ✓ |
| tool_grep_case | bashkit | 0.068 | ±0.005 | - | ✓ |
| tool_grep_case | bash | 6.992 | ±0.764 | - | ✓ |
| tool_grep_count | bashkit | 0.021 | ±0.001 | - | ✓ |
| tool_grep_count | bash | 7.706 | ±1.013 | - | ✓ |
| tool_grep_invert | bashkit | 0.021 | ±0.001 | - | ✓ |
| tool_grep_invert | bash | 7.402 | ±0.721 | - | ✓ |
| tool_grep_regex | bashkit | 0.032 | ±0.002 | - | ✓ |
| tool_grep_regex | bash | 7.170 | ±0.939 | - | ✓ |
| tool_sed_replace | bashkit | 0.145 | ±0.010 | - | ✓ |
| tool_sed_replace | bash | 7.729 | ±1.422 | - | ✓ |
| tool_sed_global | bashkit | 0.056 | ±0.003 | - | ✓ |
| tool_sed_global | bash | 9.588 | ±4.047 | - | ✓ |
| tool_sed_delete | bashkit | 0.020 | ±0.001 | - | ✓ |
| tool_sed_delete | bash | 6.775 | ±1.363 | - | ✓ |
| tool_sed_lines | bashkit | 0.020 | ±0.001 | - | ✓ |
| tool_sed_lines | bash | 8.346 | ±1.642 | - | ✓ |
| tool_sed_backrefs | bashkit | 0.075 | ±0.003 | - | ✓ |
| tool_sed_backrefs | bash | 8.603 | ±2.065 | - | ✓ |
| tool_awk_print | bashkit | 0.019 | ±0.001 | - | ✓ |
| tool_awk_print | bash | 6.460 | ±1.274 | - | ✓ |
| tool_awk_sum | bashkit | 0.056 | ±0.010 | - | ✓ |
| tool_awk_sum | bash | 7.766 | ±1.470 | - | ✓ |
| tool_awk_pattern | bashkit | 0.042 | ±0.027 | - | ✓ |
| tool_awk_pattern | bash | 7.603 | ±1.130 | - | ✓ |
| tool_awk_fieldsep | bashkit | 0.020 | ±0.001 | - | ✓ |
| tool_awk_fieldsep | bash | 7.447 | ±0.907 | - | ✓ |
| tool_awk_nf | bashkit | 0.020 | ±0.001 | - | ✓ |
| tool_awk_nf | bash | 7.982 | ±1.544 | - | ✓ |
| tool_awk_compute | bashkit | 0.020 | ±0.001 | - | ✓ |
| tool_awk_compute | bash | 11.016 | ±2.123 | - | ✓ |
| tool_jq_identity | bashkit | 0.244 | ±0.016 | - | ✓ |
| tool_jq_identity | bash | 9.415 | ±2.123 | - | ✓ |
| tool_jq_field | bashkit | 0.281 | ±0.045 | - | ✓ |
| tool_jq_field | bash | 7.224 | ±1.366 | - | ✓ |
| tool_jq_array | bashkit | 0.242 | ±0.010 | - | ✓ |
| tool_jq_array | bash | 7.107 | ±1.150 | - | ✓ |
| tool_jq_filter | bashkit | 0.250 | ±0.014 | - | ✓ |
| tool_jq_filter | bash | 6.809 | ±0.850 | - | ✓ |
| tool_jq_map | bashkit | 0.248 | ±0.013 | - | ✓ |
| tool_jq_map | bash | 7.542 | ±1.502 | - | ✓ |

### Variables

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| var_assign_simple | bashkit | 0.025 | ±0.013 | - | ✓ |
| var_assign_simple | bash | 4.579 | ±1.940 | - | ✓ |
| var_assign_many | bashkit | 0.075 | ±0.042 | - | ✓ |
| var_assign_many | bash | 7.645 | ±6.349 | - | ✓ |
| var_default | bashkit | 0.039 | ±0.001 | - | ✓ |
| var_default | bash | 4.997 | ±2.940 | - | ✓ |
| var_length | bashkit | 0.017 | ±0.001 | - | ✓ |
| var_length | bash | 4.692 | ±1.586 | - | ✓ |
| var_substring | bashkit | 0.286 | ±0.781 | - | ✓ |
| var_substring | bash | 5.495 | ±1.525 | - | ✓ |
| var_replace | bashkit | 0.049 | ±0.082 | - | ✓ |
| var_replace | bash | 8.587 | ±8.028 | - | ✓ |
| var_nested | bashkit | 0.019 | ±0.002 | - | ✓ |
| var_nested | bash | 8.399 | ±8.414 | - | ✓ |
| var_export | bashkit | 0.019 | ±0.002 | - | ✓ |
| var_export | bash | 9.118 | ±3.943 | - | ✓ |

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
