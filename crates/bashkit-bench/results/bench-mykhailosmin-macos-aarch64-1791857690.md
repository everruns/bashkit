# Bashkit Benchmark Report

## System Information

- **Moniker**: `mykhailosmin-macos-aarch64`
- **Hostname**: Mykhailos-Mini.lan
- **OS**: macos
- **Architecture**: aarch64
- **CPUs**: 12
- **Timestamp**: 1791857690
- **Iterations**: 10
- **Warmup**: 2
- **Prewarm cases**: 3

- **bash executable**: `/opt/homebrew/bin/bash (GNU Bash 5.3.20(1)-release)`

## Summary

Benchmarked 96 cases across 2 runners.

| Runner | Total Time (ms) | Avg/Case (ms) | Errors | Error Rate | Output Match |
|--------|-----------------|---------------|--------|------------|-------------|
| bashkit | 27.91 | 0.291 | 0 | 0.0% | 100.0% |
| bash | 1622.93 | 16.906 | 0 | 0.0% | 100.0% |

## Performance Comparison

**Bashkit is 58.1x faster** than bash on average.

## Results by Category

### Arithmetic

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arith_basic | bashkit | 0.032 | ±0.002 | - | ✓ |
| arith_basic | bash | 6.327 | ±0.723 | - | ✓ |
| arith_complex | bashkit | 0.033 | ±0.002 | - | ✓ |
| arith_complex | bash | 6.184 | ±0.871 | - | ✓ |
| arith_variables | bashkit | 0.036 | ±0.002 | - | ✓ |
| arith_variables | bash | 6.004 | ±0.572 | - | ✓ |
| arith_increment | bashkit | 0.035 | ±0.002 | - | ✓ |
| arith_increment | bash | 6.413 | ±0.537 | - | ✓ |
| arith_modulo | bashkit | 0.032 | ±0.002 | - | ✓ |
| arith_modulo | bash | 6.160 | ±0.473 | - | ✓ |
| arith_loop_sum | bashkit | 0.058 | ±0.007 | - | ✓ |
| arith_loop_sum | bash | 5.670 | ±0.245 | - | ✓ |

### Arrays

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arr_create | bashkit | 0.045 | ±0.021 | - | ✓ |
| arr_create | bash | 8.206 | ±1.589 | - | ✓ |
| arr_all | bashkit | 0.074 | ±0.052 | - | ✓ |
| arr_all | bash | 7.751 | ±1.710 | - | ✓ |
| arr_length | bashkit | 0.069 | ±0.052 | - | ✓ |
| arr_length | bash | 25.467 | ±10.507 | - | ✓ |
| arr_iterate | bashkit | 0.094 | ±0.080 | - | ✓ |
| arr_iterate | bash | 6.504 | ±0.578 | - | ✓ |
| arr_slice | bashkit | 0.038 | ±0.002 | - | ✓ |
| arr_slice | bash | 6.340 | ±0.973 | - | ✓ |
| arr_assign_index | bashkit | 0.045 | ±0.020 | - | ✓ |
| arr_assign_index | bash | 5.302 | ±0.346 | - | ✓ |

### Complex

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| complex_fibonacci | bashkit | 3.086 | ±0.070 | - | ✓ |
| complex_fibonacci | bash | 119.871 | ±8.967 | - | ✓ |
| complex_fibonacci_iter | bashkit | 0.090 | ±0.029 | - | ✓ |
| complex_fibonacci_iter | bash | 5.968 | ±0.551 | - | ✓ |
| complex_nested_subst | bashkit | 0.046 | ±0.001 | - | ✓ |
| complex_nested_subst | bash | 9.161 | ±1.864 | - | ✓ |
| complex_loop_compute | bashkit | 0.111 | ±0.077 | - | ✓ |
| complex_loop_compute | bash | 6.914 | ±0.895 | - | ✓ |
| complex_string_build | bashkit | 0.052 | ±0.020 | - | ✓ |
| complex_string_build | bash | 5.928 | ±0.737 | - | ✓ |
| complex_json_transform | bashkit | 0.342 | ±0.024 | - | ✓ |
| complex_json_transform | bash | 8.847 | ±0.214 | - | ✓ |
| complex_pipeline_text | bashkit | 0.087 | ±0.037 | - | ✓ |
| complex_pipeline_text | bash | 8.652 | ±0.730 | - | ✓ |

### Control

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| ctrl_if_simple | bashkit | 0.034 | ±0.002 | - | ✓ |
| ctrl_if_simple | bash | 5.804 | ±0.498 | - | ✓ |
| ctrl_if_else | bashkit | 0.035 | ±0.002 | - | ✓ |
| ctrl_if_else | bash | 5.130 | ±0.170 | - | ✓ |
| ctrl_for_list | bashkit | 0.047 | ±0.002 | - | ✓ |
| ctrl_for_list | bash | 6.058 | ±1.573 | - | ✓ |
| ctrl_for_range | bashkit | 0.060 | ±0.017 | - | ✓ |
| ctrl_for_range | bash | 11.672 | ±3.921 | - | ✓ |
| ctrl_while | bashkit | 0.073 | ±0.003 | - | ✓ |
| ctrl_while | bash | 10.071 | ±1.427 | - | ✓ |
| ctrl_case | bashkit | 0.037 | ±0.002 | - | ✓ |
| ctrl_case | bash | 12.834 | ±3.884 | - | ✓ |
| ctrl_function | bashkit | 0.038 | ±0.002 | - | ✓ |
| ctrl_function | bash | 9.903 | ±2.201 | - | ✓ |
| ctrl_function_return | bashkit | 0.132 | ±0.022 | - | ✓ |
| ctrl_function_return | bash | 9.656 | ±2.174 | - | ✓ |
| ctrl_nested_loops | bashkit | 0.062 | ±0.003 | - | ✓ |
| ctrl_nested_loops | bash | 10.697 | ±2.211 | - | ✓ |

### Io

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| io_redirect_write | bashkit | 0.052 | ±0.002 | - | ✓ |
| io_redirect_write | bash | 10.177 | ±0.555 | - | ✓ |
| io_append | bashkit | 0.067 | ±0.003 | - | ✓ |
| io_append | bash | 10.590 | ±1.575 | - | ✓ |
| io_dev_null | bashkit | 0.034 | ±0.002 | - | ✓ |
| io_dev_null | bash | 5.477 | ±0.227 | - | ✓ |
| io_stderr_redirect | bashkit | 0.035 | ±0.002 | - | ✓ |
| io_stderr_redirect | bash | 5.144 | ±0.193 | - | ✓ |
| io_read_lines | bashkit | 0.062 | ±0.002 | - | ✓ |
| io_read_lines | bash | 4.978 | ±0.231 | - | ✓ |
| io_multiline_heredoc | bashkit | 0.053 | ±0.026 | - | ✓ |
| io_multiline_heredoc | bash | 8.129 | ±0.551 | - | ✓ |

### Large

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| large_loop_1000 | bashkit | 2.298 | ±0.149 | - | ✓ |
| large_loop_1000 | bash | 7.415 | ±0.399 | - | ✓ |
| large_string_append_100 | bashkit | 0.220 | ±0.003 | - | ✓ |
| large_string_append_100 | bash | 6.272 | ±0.371 | - | ✓ |
| large_array_fill_200 | bashkit | 1.091 | ±0.252 | - | ✓ |
| large_array_fill_200 | bash | 9.059 | ±1.692 | - | ✓ |
| large_nested_loops | bashkit | 1.182 | ±0.050 | - | ✓ |
| large_nested_loops | bash | 6.600 | ±0.597 | - | ✓ |
| large_fibonacci_12 | bashkit | 8.406 | ±0.320 | - | ✓ |
| large_fibonacci_12 | bash | 335.532 | ±98.232 | - | ✓ |
| large_function_calls_500 | bashkit | 4.520 | ±0.083 | - | ✓ |
| large_function_calls_500 | bash | 418.632 | ±74.558 | - | ✓ |
| large_multiline_script | bashkit | 0.247 | ±0.021 | - | ✓ |
| large_multiline_script | bash | 5.720 | ±0.197 | - | ✓ |
| large_pipeline_chain | bashkit | 0.517 | ±0.025 | - | ✓ |
| large_pipeline_chain | bash | 8.552 | ±0.297 | - | ✓ |
| large_assoc_array | bashkit | 0.041 | ±0.002 | - | ✓ |
| large_assoc_array | bash | 5.554 | ±0.325 | - | ✓ |

### Pipes

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| pipe_simple | bashkit | 0.052 | ±0.012 | - | ✓ |
| pipe_simple | bash | 9.048 | ±0.512 | - | ✓ |
| pipe_multi | bashkit | 0.067 | ±0.002 | - | ✓ |
| pipe_multi | bash | 9.182 | ±0.604 | - | ✓ |
| pipe_command_subst | bashkit | 0.039 | ±0.002 | - | ✓ |
| pipe_command_subst | bash | 5.819 | ±0.304 | - | ✓ |
| pipe_heredoc | bashkit | 0.036 | ±0.002 | - | ✓ |
| pipe_heredoc | bash | 8.237 | ±0.811 | - | ✓ |
| pipe_herestring | bashkit | 0.043 | ±0.017 | - | ✓ |
| pipe_herestring | bash | 7.509 | ±0.242 | - | ✓ |
| pipe_discard | bashkit | 0.037 | ±0.002 | - | ✓ |
| pipe_discard | bash | 6.576 | ±0.610 | - | ✓ |

### Startup

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| startup_empty | bashkit | 0.031 | ±0.003 | - | ✓ |
| startup_empty | bash | 12.948 | ±5.328 | - | ✓ |
| startup_true | bashkit | 0.031 | ±0.002 | - | ✓ |
| startup_true | bash | 7.234 | ±0.936 | - | ✓ |
| startup_echo | bashkit | 0.033 | ±0.002 | - | ✓ |
| startup_echo | bash | 8.206 | ±1.302 | - | ✓ |
| startup_exit | bashkit | 0.031 | ±0.002 | - | ✓ |
| startup_exit | bash | 5.560 | ±0.150 | - | ✓ |

### Strings

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| str_concat | bashkit | 0.080 | ±0.070 | - | ✓ |
| str_concat | bash | 8.382 | ±1.347 | - | ✓ |
| str_printf | bashkit | 0.033 | ±0.002 | - | ✓ |
| str_printf | bash | 7.858 | ±1.358 | - | ✓ |
| str_printf_pad | bashkit | 0.044 | ±0.032 | - | ✓ |
| str_printf_pad | bash | 8.758 | ±3.044 | - | ✓ |
| str_echo_escape | bashkit | 0.032 | ±0.002 | - | ✓ |
| str_echo_escape | bash | 7.371 | ±1.451 | - | ✓ |
| str_prefix_strip | bashkit | 0.091 | ±0.030 | - | ✓ |
| str_prefix_strip | bash | 9.506 | ±1.496 | - | ✓ |
| str_suffix_strip | bashkit | 0.056 | ±0.038 | - | ✓ |
| str_suffix_strip | bash | 10.919 | ±8.362 | - | ✓ |
| str_uppercase | bashkit | 0.033 | ±0.002 | - | ✓ |
| str_uppercase | bash | 5.910 | ±1.032 | - | ✓ |
| str_lowercase | bashkit | 0.034 | ±0.002 | - | ✓ |
| str_lowercase | bash | 5.300 | ±0.229 | - | ✓ |

### Subshell

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| subshell_simple | bashkit | 0.034 | ±0.002 | - | ✓ |
| subshell_simple | bash | 6.374 | ±0.228 | - | ✓ |
| subshell_isolation | bashkit | 0.046 | ±0.010 | - | ✓ |
| subshell_isolation | bash | 8.655 | ±3.567 | - | ✓ |
| subshell_nested | bashkit | 0.051 | ±0.002 | - | ✓ |
| subshell_nested | bash | 8.987 | ±1.896 | - | ✓ |
| subshell_pipeline | bashkit | 0.044 | ±0.003 | - | ✓ |
| subshell_pipeline | bash | 8.220 | ±0.259 | - | ✓ |
| subshell_capture_loop | bashkit | 0.070 | ±0.003 | - | ✓ |
| subshell_capture_loop | bash | 9.856 | ±0.409 | - | ✓ |
| subshell_process_subst | bashkit | 0.050 | ±0.002 | - | ✓ |
| subshell_process_subst | bash | 7.768 | ±0.339 | - | ✓ |

### Tools

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| tool_grep_simple | bashkit | 0.046 | ±0.006 | - | ✓ |
| tool_grep_simple | bash | 9.305 | ±2.407 | - | ✓ |
| tool_grep_case | bashkit | 0.076 | ±0.004 | - | ✓ |
| tool_grep_case | bash | 13.231 | ±3.476 | - | ✓ |
| tool_grep_count | bashkit | 0.044 | ±0.003 | - | ✓ |
| tool_grep_count | bash | 8.754 | ±0.401 | - | ✓ |
| tool_grep_invert | bashkit | 0.045 | ±0.005 | - | ✓ |
| tool_grep_invert | bash | 8.354 | ±0.473 | - | ✓ |
| tool_grep_regex | bashkit | 0.057 | ±0.010 | - | ✓ |
| tool_grep_regex | bash | 8.983 | ±0.877 | - | ✓ |
| tool_sed_replace | bashkit | 0.051 | ±0.025 | - | ✓ |
| tool_sed_replace | bash | 8.927 | ±1.188 | - | ✓ |
| tool_sed_global | bashkit | 0.073 | ±0.038 | - | ✓ |
| tool_sed_global | bash | 8.640 | ±0.516 | - | ✓ |
| tool_sed_delete | bashkit | 0.045 | ±0.013 | - | ✓ |
| tool_sed_delete | bash | 9.103 | ±0.898 | - | ✓ |
| tool_sed_lines | bashkit | 0.039 | ±0.002 | - | ✓ |
| tool_sed_lines | bash | 8.473 | ±0.288 | - | ✓ |
| tool_sed_backrefs | bashkit | 0.050 | ±0.003 | - | ✓ |
| tool_sed_backrefs | bash | 10.082 | ±3.908 | - | ✓ |
| tool_awk_print | bashkit | 0.055 | ±0.034 | - | ✓ |
| tool_awk_print | bash | 10.103 | ±2.355 | - | ✓ |
| tool_awk_sum | bashkit | 0.046 | ±0.007 | - | ✓ |
| tool_awk_sum | bash | 8.165 | ±0.273 | - | ✓ |
| tool_awk_pattern | bashkit | 0.066 | ±0.021 | - | ✓ |
| tool_awk_pattern | bash | 8.144 | ±0.372 | - | ✓ |
| tool_awk_fieldsep | bashkit | 0.044 | ±0.005 | - | ✓ |
| tool_awk_fieldsep | bash | 8.267 | ±0.233 | - | ✓ |
| tool_awk_nf | bashkit | 0.042 | ±0.002 | - | ✓ |
| tool_awk_nf | bash | 8.563 | ±0.529 | - | ✓ |
| tool_awk_compute | bashkit | 0.057 | ±0.023 | - | ✓ |
| tool_awk_compute | bash | 8.220 | ±0.290 | - | ✓ |
| tool_jq_identity | bashkit | 0.397 | ±0.069 | - | ✓ |
| tool_jq_identity | bash | 8.762 | ±0.363 | - | ✓ |
| tool_jq_field | bashkit | 0.405 | ±0.058 | - | ✓ |
| tool_jq_field | bash | 8.411 | ±0.192 | - | ✓ |
| tool_jq_array | bashkit | 0.372 | ±0.062 | - | ✓ |
| tool_jq_array | bash | 8.487 | ±0.266 | - | ✓ |
| tool_jq_filter | bashkit | 0.345 | ±0.019 | - | ✓ |
| tool_jq_filter | bash | 8.689 | ±0.316 | - | ✓ |
| tool_jq_map | bashkit | 0.425 | ±0.068 | - | ✓ |
| tool_jq_map | bash | 9.266 | ±0.988 | - | ✓ |

### Variables

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| var_assign_simple | bashkit | 0.035 | ±0.003 | - | ✓ |
| var_assign_simple | bash | 5.827 | ±0.223 | - | ✓ |
| var_assign_many | bashkit | 0.046 | ±0.002 | - | ✓ |
| var_assign_many | bash | 5.678 | ±0.330 | - | ✓ |
| var_default | bashkit | 0.032 | ±0.002 | - | ✓ |
| var_default | bash | 6.069 | ±0.520 | - | ✓ |
| var_length | bashkit | 0.036 | ±0.005 | - | ✓ |
| var_length | bash | 6.257 | ±0.515 | - | ✓ |
| var_substring | bashkit | 0.034 | ±0.003 | - | ✓ |
| var_substring | bash | 5.352 | ±0.164 | - | ✓ |
| var_replace | bashkit | 0.036 | ±0.003 | - | ✓ |
| var_replace | bash | 5.557 | ±0.471 | - | ✓ |
| var_nested | bashkit | 0.035 | ±0.003 | - | ✓ |
| var_nested | bash | 6.430 | ±0.851 | - | ✓ |
| var_export | bashkit | 0.035 | ±0.002 | - | ✓ |
| var_export | bash | 7.326 | ±1.489 | - | ✓ |

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

