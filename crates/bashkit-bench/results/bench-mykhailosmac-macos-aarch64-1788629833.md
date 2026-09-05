# Bashkit Benchmark Report

## System Information

- **Moniker**: `mykhailosmac-macos-aarch64`
- **Hostname**: Mykhailos-Mac-mini.local
- **OS**: macos
- **Architecture**: aarch64
- **CPUs**: 12
- **Timestamp**: 1788629833
- **Iterations**: 10
- **Warmup**: 2
- **Prewarm cases**: 3

- **bash executable**: `/opt/homebrew/bin/bash (GNU Bash 5.3.15(1)-release)`

## Summary

Benchmarked 96 cases across 2 runners.

| Runner | Total Time (ms) | Avg/Case (ms) | Errors | Error Rate | Output Match |
|--------|-----------------|---------------|--------|------------|-------------|
| bashkit | 20.16 | 0.210 | 0 | 0.0% | 100.0% |
| bash | 1004.86 | 10.467 | 0 | 0.0% | 100.0% |

## Performance Comparison

**Bashkit is 49.9x faster** than bash on average.

## Results by Category

### Arithmetic

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arith_basic | bashkit | 0.017 | ±0.001 | - | ✓ |
| arith_basic | bash | 3.885 | ±0.189 | - | ✓ |
| arith_complex | bashkit | 0.018 | ±0.001 | - | ✓ |
| arith_complex | bash | 3.669 | ±0.171 | - | ✓ |
| arith_variables | bashkit | 0.020 | ±0.001 | - | ✓ |
| arith_variables | bash | 3.650 | ±0.078 | - | ✓ |
| arith_increment | bashkit | 0.018 | ±0.001 | - | ✓ |
| arith_increment | bash | 4.742 | ±0.289 | - | ✓ |
| arith_modulo | bashkit | 0.017 | ±0.001 | - | ✓ |
| arith_modulo | bash | 4.759 | ±0.416 | - | ✓ |
| arith_loop_sum | bashkit | 0.038 | ±0.002 | - | ✓ |
| arith_loop_sum | bash | 7.080 | ±4.903 | - | ✓ |

### Arrays

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| arr_create | bashkit | 0.018 | ±0.002 | - | ✓ |
| arr_create | bash | 4.080 | ±0.272 | - | ✓ |
| arr_all | bashkit | 0.018 | ±0.001 | - | ✓ |
| arr_all | bash | 3.889 | ±0.217 | - | ✓ |
| arr_length | bashkit | 0.018 | ±0.001 | - | ✓ |
| arr_length | bash | 3.755 | ±0.121 | - | ✓ |
| arr_iterate | bashkit | 0.023 | ±0.001 | - | ✓ |
| arr_iterate | bash | 4.048 | ±0.378 | - | ✓ |
| arr_slice | bashkit | 0.020 | ±0.001 | - | ✓ |
| arr_slice | bash | 4.457 | ±0.457 | - | ✓ |
| arr_assign_index | bashkit | 0.020 | ±0.001 | - | ✓ |
| arr_assign_index | bash | 4.110 | ±0.324 | - | ✓ |

### Complex

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| complex_fibonacci | bashkit | 2.176 | ±0.087 | - | ✓ |
| complex_fibonacci | bash | 78.101 | ±5.523 | - | ✓ |
| complex_fibonacci_iter | bashkit | 0.065 | ±0.037 | - | ✓ |
| complex_fibonacci_iter | bash | 4.503 | ±0.162 | - | ✓ |
| complex_nested_subst | bashkit | 0.025 | ±0.002 | - | ✓ |
| complex_nested_subst | bash | 5.276 | ±0.266 | - | ✓ |
| complex_loop_compute | bashkit | 0.049 | ±0.001 | - | ✓ |
| complex_loop_compute | bash | 3.846 | ±0.095 | - | ✓ |
| complex_string_build | bashkit | 0.033 | ±0.013 | - | ✓ |
| complex_string_build | bash | 3.800 | ±0.175 | - | ✓ |
| complex_json_transform | bashkit | 0.238 | ±0.015 | - | ✓ |
| complex_json_transform | bash | 6.588 | ±0.281 | - | ✓ |
| complex_pipeline_text | bashkit | 0.066 | ±0.003 | - | ✓ |
| complex_pipeline_text | bash | 6.123 | ±0.255 | - | ✓ |

### Control

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| ctrl_if_simple | bashkit | 0.019 | ±0.002 | - | ✓ |
| ctrl_if_simple | bash | 4.304 | ±0.264 | - | ✓ |
| ctrl_if_else | bashkit | 0.020 | ±0.001 | - | ✓ |
| ctrl_if_else | bash | 4.119 | ±0.201 | - | ✓ |
| ctrl_for_list | bashkit | 0.029 | ±0.001 | - | ✓ |
| ctrl_for_list | bash | 4.081 | ±0.274 | - | ✓ |
| ctrl_for_range | bashkit | 0.032 | ±0.002 | - | ✓ |
| ctrl_for_range | bash | 3.812 | ±0.138 | - | ✓ |
| ctrl_while | bashkit | 0.049 | ±0.001 | - | ✓ |
| ctrl_while | bash | 3.767 | ±0.143 | - | ✓ |
| ctrl_case | bashkit | 0.021 | ±0.001 | - | ✓ |
| ctrl_case | bash | 3.801 | ±0.201 | - | ✓ |
| ctrl_function | bashkit | 0.021 | ±0.002 | - | ✓ |
| ctrl_function | bash | 3.774 | ±0.158 | - | ✓ |
| ctrl_function_return | bashkit | 0.025 | ±0.001 | - | ✓ |
| ctrl_function_return | bash | 4.332 | ±0.142 | - | ✓ |
| ctrl_nested_loops | bashkit | 0.039 | ±0.001 | - | ✓ |
| ctrl_nested_loops | bash | 3.759 | ±0.154 | - | ✓ |

### Io

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| io_redirect_write | bashkit | 0.030 | ±0.001 | - | ✓ |
| io_redirect_write | bash | 6.930 | ±0.280 | - | ✓ |
| io_append | bashkit | 0.037 | ±0.001 | - | ✓ |
| io_append | bash | 6.978 | ±0.210 | - | ✓ |
| io_dev_null | bashkit | 0.018 | ±0.002 | - | ✓ |
| io_dev_null | bash | 4.860 | ±0.368 | - | ✓ |
| io_stderr_redirect | bashkit | 0.019 | ±0.003 | - | ✓ |
| io_stderr_redirect | bash | 5.060 | ±0.242 | - | ✓ |
| io_read_lines | bashkit | 0.040 | ±0.001 | - | ✓ |
| io_read_lines | bash | 5.009 | ±0.395 | - | ✓ |
| io_multiline_heredoc | bashkit | 0.024 | ±0.001 | - | ✓ |
| io_multiline_heredoc | bash | 7.128 | ±0.172 | - | ✓ |

### Large

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| large_loop_1000 | bashkit | 2.406 | ±0.052 | - | ✓ |
| large_loop_1000 | bash | 5.418 | ±0.147 | - | ✓ |
| large_string_append_100 | bashkit | 0.178 | ±0.003 | - | ✓ |
| large_string_append_100 | bash | 4.136 | ±0.179 | - | ✓ |
| large_array_fill_200 | bashkit | 0.741 | ±0.016 | - | ✓ |
| large_array_fill_200 | bash | 4.116 | ±0.170 | - | ✓ |
| large_nested_loops | bashkit | 1.083 | ±0.017 | - | ✓ |
| large_nested_loops | bash | 4.593 | ±0.150 | - | ✓ |
| large_fibonacci_12 | bashkit | 6.139 | ±0.176 | - | ✓ |
| large_fibonacci_12 | bash | 185.502 | ±3.336 | - | ✓ |
| large_function_calls_500 | bashkit | 2.966 | ±0.049 | - | ✓ |
| large_function_calls_500 | bash | 245.854 | ±11.774 | - | ✓ |
| large_multiline_script | bashkit | 0.210 | ±0.009 | - | ✓ |
| large_multiline_script | bash | 4.210 | ±0.251 | - | ✓ |
| large_pipeline_chain | bashkit | 0.464 | ±0.005 | - | ✓ |
| large_pipeline_chain | bash | 6.765 | ±0.174 | - | ✓ |
| large_assoc_array | bashkit | 0.023 | ±0.001 | - | ✓ |
| large_assoc_array | bash | 4.095 | ±0.369 | - | ✓ |

### Pipes

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| pipe_simple | bashkit | 0.024 | ±0.002 | - | ✓ |
| pipe_simple | bash | 6.336 | ±0.376 | - | ✓ |
| pipe_multi | bashkit | 0.032 | ±0.001 | - | ✓ |
| pipe_multi | bash | 6.601 | ±0.315 | - | ✓ |
| pipe_command_subst | bashkit | 0.020 | ±0.001 | - | ✓ |
| pipe_command_subst | bash | 4.583 | ±0.198 | - | ✓ |
| pipe_heredoc | bashkit | 0.021 | ±0.001 | - | ✓ |
| pipe_heredoc | bash | 5.435 | ±0.204 | - | ✓ |
| pipe_herestring | bashkit | 0.020 | ±0.001 | - | ✓ |
| pipe_herestring | bash | 7.253 | ±0.318 | - | ✓ |
| pipe_discard | bashkit | 0.020 | ±0.001 | - | ✓ |
| pipe_discard | bash | 5.460 | ±0.509 | - | ✓ |

### Startup

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| startup_empty | bashkit | 0.025 | ±0.013 | - | ✓ |
| startup_empty | bash | 4.451 | ±0.248 | - | ✓ |
| startup_true | bashkit | 0.016 | ±0.001 | - | ✓ |
| startup_true | bash | 5.086 | ±0.748 | - | ✓ |
| startup_echo | bashkit | 0.017 | ±0.001 | - | ✓ |
| startup_echo | bash | 6.145 | ±1.645 | - | ✓ |
| startup_exit | bashkit | 0.016 | ±0.001 | - | ✓ |
| startup_exit | bash | 4.212 | ±0.219 | - | ✓ |

### Strings

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| str_concat | bashkit | 0.018 | ±0.001 | - | ✓ |
| str_concat | bash | 3.773 | ±0.145 | - | ✓ |
| str_printf | bashkit | 0.019 | ±0.004 | - | ✓ |
| str_printf | bash | 3.752 | ±0.146 | - | ✓ |
| str_printf_pad | bashkit | 0.017 | ±0.001 | - | ✓ |
| str_printf_pad | bash | 3.815 | ±0.315 | - | ✓ |
| str_echo_escape | bashkit | 0.023 | ±0.016 | - | ✓ |
| str_echo_escape | bash | 4.852 | ±0.319 | - | ✓ |
| str_prefix_strip | bashkit | 0.019 | ±0.001 | - | ✓ |
| str_prefix_strip | bash | 5.303 | ±0.289 | - | ✓ |
| str_suffix_strip | bashkit | 0.024 | ±0.009 | - | ✓ |
| str_suffix_strip | bash | 5.150 | ±0.336 | - | ✓ |
| str_uppercase | bashkit | 0.018 | ±0.003 | - | ✓ |
| str_uppercase | bash | 4.578 | ±0.256 | - | ✓ |
| str_lowercase | bashkit | 0.018 | ±0.001 | - | ✓ |
| str_lowercase | bash | 4.178 | ±0.260 | - | ✓ |

### Subshell

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| subshell_simple | bashkit | 0.016 | ±0.000 | - | ✓ |
| subshell_simple | bash | 4.253 | ±0.197 | - | ✓ |
| subshell_isolation | bashkit | 0.021 | ±0.001 | - | ✓ |
| subshell_isolation | bash | 4.372 | ±0.221 | - | ✓ |
| subshell_nested | bashkit | 0.027 | ±0.002 | - | ✓ |
| subshell_nested | bash | 7.526 | ±2.128 | - | ✓ |
| subshell_pipeline | bashkit | 0.019 | ±0.001 | - | ✓ |
| subshell_pipeline | bash | 6.124 | ±0.155 | - | ✓ |
| subshell_capture_loop | bashkit | 0.044 | ±0.001 | - | ✓ |
| subshell_capture_loop | bash | 6.618 | ±0.179 | - | ✓ |
| subshell_process_subst | bashkit | 0.029 | ±0.001 | - | ✓ |
| subshell_process_subst | bash | 4.845 | ±0.086 | - | ✓ |

### Tools

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| tool_grep_simple | bashkit | 0.022 | ±0.001 | - | ✓ |
| tool_grep_simple | bash | 7.946 | ±0.617 | - | ✓ |
| tool_grep_case | bashkit | 0.080 | ±0.026 | - | ✓ |
| tool_grep_case | bash | 7.619 | ±0.454 | - | ✓ |
| tool_grep_count | bashkit | 0.023 | ±0.003 | - | ✓ |
| tool_grep_count | bash | 7.224 | ±0.281 | - | ✓ |
| tool_grep_invert | bashkit | 0.022 | ±0.001 | - | ✓ |
| tool_grep_invert | bash | 6.839 | ±0.242 | - | ✓ |
| tool_grep_regex | bashkit | 0.032 | ±0.002 | - | ✓ |
| tool_grep_regex | bash | 6.877 | ±0.383 | - | ✓ |
| tool_sed_replace | bashkit | 0.056 | ±0.003 | - | ✓ |
| tool_sed_replace | bash | 6.468 | ±0.330 | - | ✓ |
| tool_sed_global | bashkit | 0.070 | ±0.048 | - | ✓ |
| tool_sed_global | bash | 6.950 | ±0.369 | - | ✓ |
| tool_sed_delete | bashkit | 0.021 | ±0.001 | - | ✓ |
| tool_sed_delete | bash | 6.849 | ±0.489 | - | ✓ |
| tool_sed_lines | bashkit | 0.019 | ±0.001 | - | ✓ |
| tool_sed_lines | bash | 6.717 | ±0.307 | - | ✓ |
| tool_sed_backrefs | bashkit | 0.075 | ±0.004 | - | ✓ |
| tool_sed_backrefs | bash | 6.860 | ±0.308 | - | ✓ |
| tool_awk_print | bashkit | 0.020 | ±0.002 | - | ✓ |
| tool_awk_print | bash | 6.601 | ±0.401 | - | ✓ |
| tool_awk_sum | bashkit | 0.023 | ±0.001 | - | ✓ |
| tool_awk_sum | bash | 5.832 | ±0.238 | - | ✓ |
| tool_awk_pattern | bashkit | 0.031 | ±0.002 | - | ✓ |
| tool_awk_pattern | bash | 6.592 | ±0.746 | - | ✓ |
| tool_awk_fieldsep | bashkit | 0.021 | ±0.001 | - | ✓ |
| tool_awk_fieldsep | bash | 7.455 | ±0.399 | - | ✓ |
| tool_awk_nf | bashkit | 0.024 | ±0.007 | - | ✓ |
| tool_awk_nf | bash | 7.489 | ±0.266 | - | ✓ |
| tool_awk_compute | bashkit | 0.021 | ±0.001 | - | ✓ |
| tool_awk_compute | bash | 7.186 | ±0.271 | - | ✓ |
| tool_jq_identity | bashkit | 0.289 | ±0.087 | - | ✓ |
| tool_jq_identity | bash | 7.583 | ±0.476 | - | ✓ |
| tool_jq_field | bashkit | 0.260 | ±0.066 | - | ✓ |
| tool_jq_field | bash | 8.001 | ±0.323 | - | ✓ |
| tool_jq_array | bashkit | 0.324 | ±0.122 | - | ✓ |
| tool_jq_array | bash | 8.071 | ±0.232 | - | ✓ |
| tool_jq_filter | bashkit | 0.230 | ±0.010 | - | ✓ |
| tool_jq_filter | bash | 7.965 | ±0.399 | - | ✓ |
| tool_jq_map | bashkit | 0.287 | ±0.038 | - | ✓ |
| tool_jq_map | bash | 7.842 | ±0.203 | - | ✓ |

### Variables

| Benchmark | Runner | Mean (ms) | StdDev | Errors | Match |
|-----------|--------|-----------|--------|--------|-------|
| var_assign_simple | bashkit | 0.019 | ±0.001 | - | ✓ |
| var_assign_simple | bash | 4.283 | ±0.256 | - | ✓ |
| var_assign_many | bashkit | 0.029 | ±0.004 | - | ✓ |
| var_assign_many | bash | 4.241 | ±0.347 | - | ✓ |
| var_default | bashkit | 0.017 | ±0.001 | - | ✓ |
| var_default | bash | 4.130 | ±0.178 | - | ✓ |
| var_length | bashkit | 0.017 | ±0.001 | - | ✓ |
| var_length | bash | 4.207 | ±0.230 | - | ✓ |
| var_substring | bashkit | 0.018 | ±0.001 | - | ✓ |
| var_substring | bash | 4.062 | ±0.198 | - | ✓ |
| var_replace | bashkit | 0.019 | ±0.001 | - | ✓ |
| var_replace | bash | 3.855 | ±0.131 | - | ✓ |
| var_nested | bashkit | 0.019 | ±0.001 | - | ✓ |
| var_nested | bash | 3.849 | ±0.181 | - | ✓ |
| var_export | bashkit | 0.019 | ±0.001 | - | ✓ |
| var_export | bash | 3.803 | ±0.188 | - | ✓ |

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

