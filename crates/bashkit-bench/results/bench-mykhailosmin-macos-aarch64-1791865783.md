# Bashkit Benchmark Report

## System Information

- **Moniker**: `mykhailosmin-macos-aarch64`
- **Hostname**: Mykhailos-Mini.lan
- **OS**: macos
- **Architecture**: aarch64
- **CPUs**: 12
- **Timestamp**: 1791865783
- **Iterations**: 10
- **Warmup**: 2
- **Prewarm cases**: 3

- **bash executable**: `/opt/homebrew/bin/bash (GNU Bash 5.3.20(1)-release)`

## Summary

Benchmarked 96 cases across 2 runners.

| Runner | Total Time (ms) | Avg/Case (ms) | Errors | Error Rate | Output Match |
|--------|-----------------|---------------|--------|------------|-------------|
| bashkit | 103.20 | 1.075 | 0 | 0.0% | 100.0% |
| bash | 2463.28 | 25.659 | 0 | 0.0% | 100.0% |

## Performance Comparison

**Bashkit is 23.9x faster** than bash on average.

## Results by Category

### Arithmetic

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arith_basic | bashkit | 0.033 | ±0.001 | - | ✓ |
| arith_basic | bash | 4.974 | ±0.256 | - | ✓ |
| arith_complex | bashkit | 0.032 | ±0.002 | - | ✓ |
| arith_complex | bash | 4.819 | ±0.302 | - | ✓ |
| arith_variables | bashkit | 0.036 | ±0.002 | - | ✓ |
| arith_variables | bash | 4.629 | ±0.359 | - | ✓ |
| arith_increment | bashkit | 0.035 | ±0.002 | - | ✓ |
| arith_increment | bash | 4.540 | ±0.348 | - | ✓ |
| arith_modulo | bashkit | 0.031 | ±0.001 | - | ✓ |
| arith_modulo | bash | 4.750 | ±0.145 | - | ✓ |
| arith_loop_sum | bashkit | 0.054 | ±0.002 | - | ✓ |
| arith_loop_sum | bash | 4.896 | ±0.291 | - | ✓ |

### Arrays

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arr_create | bashkit | 0.039 | ±0.003 | - | ✓ |
| arr_create | bash | 10.290 | ±5.096 | - | ✓ |
| arr_all | bashkit | 0.037 | ±0.003 | - | ✓ |
| arr_all | bash | 7.469 | ±2.376 | - | ✓ |
| arr_length | bashkit | 0.039 | ±0.002 | - | ✓ |
| arr_length | bash | 6.669 | ±1.146 | - | ✓ |
| arr_iterate | bashkit | 0.042 | ±0.002 | - | ✓ |
| arr_iterate | bash | 6.147 | ±0.987 | - | ✓ |
| arr_slice | bashkit | 0.073 | ±0.051 | - | ✓ |
| arr_slice | bash | 6.770 | ±1.501 | - | ✓ |
| arr_assign_index | bashkit | 0.040 | ±0.002 | - | ✓ |
| arr_assign_index | bash | 7.287 | ±1.317 | - | ✓ |

### Complex

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| complex_fibonacci | bashkit | 4.606 | ±1.432 | - | ✓ |
| complex_fibonacci | bash | 213.997 | ±35.990 | - | ✓ |
| complex_fibonacci_iter | bashkit | 0.126 | ±0.091 | - | ✓ |
| complex_fibonacci_iter | bash | 5.772 | ±0.385 | - | ✓ |
| complex_nested_subst | bashkit | 0.050 | ±0.003 | - | ✓ |
| complex_nested_subst | bash | 9.608 | ±1.164 | - | ✓ |
| complex_loop_compute | bashkit | 0.074 | ±0.003 | - | ✓ |
| complex_loop_compute | bash | 7.797 | ±1.316 | - | ✓ |
| complex_string_build | bashkit | 0.088 | ±0.056 | - | ✓ |
| complex_string_build | bash | 6.300 | ±1.059 | - | ✓ |
| complex_json_transform | bashkit | 0.396 | ±0.059 | - | ✓ |
| complex_json_transform | bash | 10.536 | ±1.045 | - | ✓ |
| complex_pipeline_text | bashkit | 0.108 | ±0.067 | - | ✓ |
| complex_pipeline_text | bash | 11.530 | ±1.838 | - | ✓ |

### Control

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| ctrl_if_simple | bashkit | 0.034 | ±0.001 | - | ✓ |
| ctrl_if_simple | bash | 7.276 | ±4.271 | - | ✓ |
| ctrl_if_else | bashkit | 0.036 | ±0.002 | - | ✓ |
| ctrl_if_else | bash | 13.713 | ±5.608 | - | ✓ |
| ctrl_for_list | bashkit | 0.062 | ±0.023 | - | ✓ |
| ctrl_for_list | bash | 13.661 | ±4.050 | - | ✓ |
| ctrl_for_range | bashkit | 0.055 | ±0.003 | - | ✓ |
| ctrl_for_range | bash | 7.842 | ±2.122 | - | ✓ |
| ctrl_while | bashkit | 0.078 | ±0.010 | - | ✓ |
| ctrl_while | bash | 11.540 | ±1.850 | - | ✓ |
| ctrl_case | bashkit | 0.037 | ±0.002 | - | ✓ |
| ctrl_case | bash | 15.494 | ±4.938 | - | ✓ |
| ctrl_function | bashkit | 0.055 | ±0.043 | - | ✓ |
| ctrl_function | bash | 7.837 | ±1.868 | - | ✓ |
| ctrl_function_return | bashkit | 0.046 | ±0.003 | - | ✓ |
| ctrl_function_return | bash | 7.614 | ±1.520 | - | ✓ |
| ctrl_nested_loops | bashkit | 0.065 | ±0.004 | - | ✓ |
| ctrl_nested_loops | bash | 7.723 | ±2.009 | - | ✓ |

### Io

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| io_redirect_write | bashkit | 0.054 | ±0.003 | - | ✓ |
| io_redirect_write | bash | 10.175 | ±1.878 | - | ✓ |
| io_append | bashkit | 0.065 | ±0.003 | - | ✓ |
| io_append | bash | 9.470 | ±0.283 | - | ✓ |
| io_dev_null | bashkit | 0.035 | ±0.002 | - | ✓ |
| io_dev_null | bash | 5.183 | ±0.382 | - | ✓ |
| io_stderr_redirect | bashkit | 0.034 | ±0.001 | - | ✓ |
| io_stderr_redirect | bash | 5.186 | ±0.219 | - | ✓ |
| io_read_lines | bashkit | 0.063 | ±0.002 | - | ✓ |
| io_read_lines | bash | 5.298 | ±0.225 | - | ✓ |
| io_multiline_heredoc | bashkit | 0.042 | ±0.003 | - | ✓ |
| io_multiline_heredoc | bash | 7.790 | ±0.329 | - | ✓ |

### Large

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| large_loop_1000 | bashkit | 2.553 | ±0.208 | - | ✓ |
| large_loop_1000 | bash | 12.428 | ±4.617 | - | ✓ |
| large_string_append_100 | bashkit | 0.281 | ±0.064 | - | ✓ |
| large_string_append_100 | bash | 6.374 | ±0.600 | - | ✓ |
| large_array_fill_200 | bashkit | 1.016 | ±0.024 | - | ✓ |
| large_array_fill_200 | bash | 6.461 | ±1.183 | - | ✓ |
| large_nested_loops | bashkit | 1.181 | ±0.046 | - | ✓ |
| large_nested_loops | bash | 7.134 | ±0.878 | - | ✓ |
| large_fibonacci_12 | bashkit | 80.879 | ±72.233 | - | ✓ |
| large_fibonacci_12 | bash | 729.946 | ±126.332 | - | ✓ |
| large_function_calls_500 | bashkit | 5.003 | ±0.379 | - | ✓ |
| large_function_calls_500 | bash | 716.907 | ±270.256 | - | ✓ |
| large_multiline_script | bashkit | 0.228 | ±0.004 | - | ✓ |
| large_multiline_script | bash | 6.167 | ±0.618 | - | ✓ |
| large_pipeline_chain | bashkit | 0.580 | ±0.038 | - | ✓ |
| large_pipeline_chain | bash | 9.252 | ±1.354 | - | ✓ |
| large_assoc_array | bashkit | 0.046 | ±0.002 | - | ✓ |
| large_assoc_array | bash | 5.750 | ±0.139 | - | ✓ |

### Pipes

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| pipe_simple | bashkit | 0.049 | ±0.003 | - | ✓ |
| pipe_simple | bash | 15.010 | ±2.865 | - | ✓ |
| pipe_multi | bashkit | 0.140 | ±0.076 | - | ✓ |
| pipe_multi | bash | 11.099 | ±1.613 | - | ✓ |
| pipe_command_subst | bashkit | 0.038 | ±0.002 | - | ✓ |
| pipe_command_subst | bash | 6.916 | ±0.945 | - | ✓ |
| pipe_heredoc | bashkit | 0.036 | ±0.002 | - | ✓ |
| pipe_heredoc | bash | 8.003 | ±0.541 | - | ✓ |
| pipe_herestring | bashkit | 0.045 | ±0.011 | - | ✓ |
| pipe_herestring | bash | 8.930 | ±1.153 | - | ✓ |
| pipe_discard | bashkit | 0.041 | ±0.002 | - | ✓ |
| pipe_discard | bash | 8.403 | ±0.967 | - | ✓ |

### Startup

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| startup_empty | bashkit | 0.030 | ±0.001 | - | ✓ |
| startup_empty | bash | 4.658 | ±0.230 | - | ✓ |
| startup_true | bashkit | 0.030 | ±0.001 | - | ✓ |
| startup_true | bash | 5.096 | ±0.579 | - | ✓ |
| startup_echo | bashkit | 0.032 | ±0.002 | - | ✓ |
| startup_echo | bash | 4.849 | ±0.126 | - | ✓ |
| startup_exit | bashkit | 0.031 | ±0.002 | - | ✓ |
| startup_exit | bash | 4.719 | ±0.266 | - | ✓ |

### Strings

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| str_concat | bashkit | 0.068 | ±0.048 | - | ✓ |
| str_concat | bash | 7.650 | ±1.647 | - | ✓ |
| str_printf | bashkit | 0.035 | ±0.002 | - | ✓ |
| str_printf | bash | 8.740 | ±2.011 | - | ✓ |
| str_printf_pad | bashkit | 0.112 | ±0.056 | - | ✓ |
| str_printf_pad | bash | 7.947 | ±2.054 | - | ✓ |
| str_echo_escape | bashkit | 0.032 | ±0.002 | - | ✓ |
| str_echo_escape | bash | 10.109 | ±2.861 | - | ✓ |
| str_prefix_strip | bashkit | 0.099 | ±0.044 | - | ✓ |
| str_prefix_strip | bash | 8.303 | ±1.163 | - | ✓ |
| str_suffix_strip | bashkit | 0.091 | ±0.144 | - | ✓ |
| str_suffix_strip | bash | 8.795 | ±3.834 | - | ✓ |
| str_uppercase | bashkit | 0.073 | ±0.045 | - | ✓ |
| str_uppercase | bash | 14.037 | ±3.623 | - | ✓ |
| str_lowercase | bashkit | 0.038 | ±0.004 | - | ✓ |
| str_lowercase | bash | 11.237 | ±5.046 | - | ✓ |

### Subshell

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| subshell_simple | bashkit | 0.038 | ±0.002 | - | ✓ |
| subshell_simple | bash | 24.509 | ±19.111 | - | ✓ |
| subshell_isolation | bashkit | 0.040 | ±0.002 | - | ✓ |
| subshell_isolation | bash | 12.493 | ±8.314 | - | ✓ |
| subshell_nested | bashkit | 0.059 | ±0.013 | - | ✓ |
| subshell_nested | bash | 8.630 | ±0.663 | - | ✓ |
| subshell_pipeline | bashkit | 0.047 | ±0.003 | - | ✓ |
| subshell_pipeline | bash | 8.217 | ±0.538 | - | ✓ |
| subshell_capture_loop | bashkit | 0.069 | ±0.002 | - | ✓ |
| subshell_capture_loop | bash | 9.114 | ±0.187 | - | ✓ |
| subshell_process_subst | bashkit | 0.051 | ±0.002 | - | ✓ |
| subshell_process_subst | bash | 7.276 | ±0.362 | - | ✓ |

### Tools

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| tool_grep_simple | bashkit | 0.049 | ±0.002 | - | ✓ |
| tool_grep_simple | bash | 12.741 | ±2.599 | - | ✓ |
| tool_grep_case | bashkit | 0.125 | ±0.079 | - | ✓ |
| tool_grep_case | bash | 11.014 | ±2.471 | - | ✓ |
| tool_grep_count | bashkit | 0.048 | ±0.002 | - | ✓ |
| tool_grep_count | bash | 9.383 | ±0.943 | - | ✓ |
| tool_grep_invert | bashkit | 0.045 | ±0.003 | - | ✓ |
| tool_grep_invert | bash | 9.337 | ±0.801 | - | ✓ |
| tool_grep_regex | bashkit | 0.053 | ±0.003 | - | ✓ |
| tool_grep_regex | bash | 8.952 | ±1.103 | - | ✓ |
| tool_sed_replace | bashkit | 0.042 | ±0.006 | - | ✓ |
| tool_sed_replace | bash | 9.035 | ±1.272 | - | ✓ |
| tool_sed_global | bashkit | 0.039 | ±0.002 | - | ✓ |
| tool_sed_global | bash | 9.746 | ±1.783 | - | ✓ |
| tool_sed_delete | bashkit | 0.040 | ±0.002 | - | ✓ |
| tool_sed_delete | bash | 7.876 | ±0.529 | - | ✓ |
| tool_sed_lines | bashkit | 0.039 | ±0.002 | - | ✓ |
| tool_sed_lines | bash | 11.444 | ±1.915 | - | ✓ |
| tool_sed_backrefs | bashkit | 0.049 | ±0.003 | - | ✓ |
| tool_sed_backrefs | bash | 9.775 | ±1.063 | - | ✓ |
| tool_awk_print | bashkit | 0.043 | ±0.002 | - | ✓ |
| tool_awk_print | bash | 9.328 | ±1.389 | - | ✓ |
| tool_awk_sum | bashkit | 0.045 | ±0.002 | - | ✓ |
| tool_awk_sum | bash | 9.325 | ±1.047 | - | ✓ |
| tool_awk_pattern | bashkit | 0.068 | ±0.020 | - | ✓ |
| tool_awk_pattern | bash | 8.156 | ±0.713 | - | ✓ |
| tool_awk_fieldsep | bashkit | 0.080 | ±0.033 | - | ✓ |
| tool_awk_fieldsep | bash | 10.838 | ±1.506 | - | ✓ |
| tool_awk_nf | bashkit | 0.047 | ±0.003 | - | ✓ |
| tool_awk_nf | bash | 10.236 | ±1.523 | - | ✓ |
| tool_awk_compute | bashkit | 0.049 | ±0.006 | - | ✓ |
| tool_awk_compute | bash | 15.517 | ±3.877 | - | ✓ |
| tool_jq_identity | bashkit | 0.380 | ±0.057 | - | ✓ |
| tool_jq_identity | bash | 11.841 | ±1.602 | - | ✓ |
| tool_jq_field | bashkit | 0.737 | ±0.260 | - | ✓ |
| tool_jq_field | bash | 14.556 | ±3.001 | - | ✓ |
| tool_jq_array | bashkit | 0.377 | ±0.035 | - | ✓ |
| tool_jq_array | bash | 13.113 | ±1.604 | - | ✓ |
| tool_jq_filter | bashkit | 0.416 | ±0.134 | - | ✓ |
| tool_jq_filter | bash | 12.378 | ±2.075 | - | ✓ |
| tool_jq_map | bashkit | 0.357 | ±0.018 | - | ✓ |
| tool_jq_map | bash | 12.705 | ±1.862 | - | ✓ |

### Variables

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| var_assign_simple | bashkit | 0.035 | ±0.002 | - | ✓ |
| var_assign_simple | bash | 4.907 | ±0.236 | - | ✓ |
| var_assign_many | bashkit | 0.045 | ±0.002 | - | ✓ |
| var_assign_many | bash | 4.208 | ±0.138 | - | ✓ |
| var_default | bashkit | 0.031 | ±0.002 | - | ✓ |
| var_default | bash | 4.172 | ±0.178 | - | ✓ |
| var_length | bashkit | 0.031 | ±0.002 | - | ✓ |
| var_length | bash | 4.629 | ±0.363 | - | ✓ |
| var_substring | bashkit | 0.035 | ±0.005 | - | ✓ |
| var_substring | bash | 4.321 | ±0.154 | - | ✓ |
| var_replace | bashkit | 0.035 | ±0.003 | - | ✓ |
| var_replace | bash | 4.368 | ±0.186 | - | ✓ |
| var_nested | bashkit | 0.034 | ±0.002 | - | ✓ |
| var_nested | bash | 4.845 | ±0.414 | - | ✓ |
| var_export | bashkit | 0.036 | ±0.003 | - | ✓ |
| var_export | bash | 4.791 | ±0.206 | - | ✓ |

## Runner Descriptions

| Runner | Type | Description |
|--------|------|-------------|
| bashkit | in-process | Rust library call, no fork/exec |
| bashkit-cli | subprocess | bashkit binary, new process per run |
| bashkit-js | persistent child | Node.js + @everruns/bashkit, warm interpreter |
| bashkit-py | persistent child | Python + bashkit package, warm interpreter |
| bash | subprocess | PATH-selected Bash >=4, new process per run |
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

