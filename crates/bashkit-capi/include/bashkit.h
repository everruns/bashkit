#ifndef BASHKIT_H
#define BASHKIT_H

#include <stddef.h>
#include <stdint.h>

#if defined(_WIN32) && !defined(BASHKIT_STATIC)
#define BASHKIT_API __declspec(dllimport)
#elif defined(__GNUC__)
#define BASHKIT_API __attribute__((visibility("default")))
#else
#define BASHKIT_API
#endif

#ifdef __cplusplus
extern "C" {
#endif

#define BASHKIT_ABI_VERSION_1 1u
#define BASHKIT_MAX_CONFIG_BYTES 10000000u
#define BASHKIT_RESULT_STDOUT_TRUNCATED (1u << 0)
#define BASHKIT_RESULT_STDERR_TRUNCATED (1u << 1)

typedef struct bashkit Bashkit;
typedef struct bashkit_result BashkitResult;
typedef struct bashkit_error BashkitError;
typedef struct bashkit_buffer BashkitBuffer;
typedef struct bashkit_runtime_call BashkitRuntimeCall;

typedef struct {
    const uint8_t *ptr;
    size_t len;
} BashkitBytes;

typedef uint32_t BashkitStatus;
#define BASHKIT_OK 0u
#define BASHKIT_INVALID_ARGUMENT 1u
#define BASHKIT_INVALID_UTF8 2u
#define BASHKIT_INVALID_CONFIG 3u
#define BASHKIT_EXECUTION_ERROR 4u
#define BASHKIT_IO_ERROR 5u
#define BASHKIT_UNSUPPORTED 6u
#define BASHKIT_CANCELLED 7u
#define BASHKIT_BUSY 8u
#define BASHKIT_MAX_RUNTIME_VALUE_BYTES 10000000u
#define BASHKIT_INTERNAL_ERROR 255u

/* Static views returned by these functions remain valid for process lifetime. */
BASHKIT_API uint32_t bashkit_abi_version(void);
BASHKIT_API BashkitBytes bashkit_version(void);
BASHKIT_API BashkitBytes bashkit_capabilities_json(void);

/* A nonzero shell exit code is a successful ABI call; inspect the result. */
BASHKIT_API BashkitStatus bashkit_create_default(
    Bashkit **out_bash,
    BashkitError **out_error);
BASHKIT_API BashkitStatus bashkit_create_json(
    BashkitBytes config_json,
    Bashkit **out_bash,
    BashkitError **out_error);
BASHKIT_API void bashkit_free(Bashkit *bash);
BASHKIT_API BashkitStatus bashkit_execute(
    Bashkit *bash,
    BashkitBytes script,
    BashkitResult **out_result,
    BashkitError **out_error);

/* Embedded runtime callbacks (capability runtime-host-functions).
   Config schema v1 adds python/typescript booleans, external_functions and
   typescript_external_functions arrays. Names require an enabled runtime and
   this constructor's non-NULL handler. One dispatcher receives both languages.

   handler runs on the execution caller thread (buffered or streaming) and must
   return promptly.
   It receives OWNERSHIP of call: respond/fail and free it once, or transfer it
   to another thread and return. Request JSON contains language, name, args,
   and kwargs for Python. JSON values are bounded to 64 nesting levels, finite
   numbers and signed 64-bit integers. No arbitrary language objects.

   respond_json copies input; invalid input leaves the call unanswered. Exactly
   one response is accepted. Late replies return CANCELLED. Poll cancelled to
   stop host I/O after timeout/cancel; free an unanswered call to fail promptly.
   Every call must be freed, including cancelled calls. View validity ends at
   call_free. Respond/query may run concurrently, but never race call_free.

   Blocking handle APIs called from a handler return BUSY, including on another
   handle (nested current-thread runtimes are forbidden). Callback-enabled
   handles return BUSY for concurrent blocking operations. cancel/clear_cancel
   and runtime_call_* stay available. Queue other work after handler returns.

   handler/user_data must remain valid through handle lifetime and executions.
   Never unwind/longjmp through the callback or free a Bashkit during use. The
   shared library must stay loaded until all handles/results/calls are freed. */
typedef void (*BashkitRuntimeHandlerV1)(void *user_data, BashkitRuntimeCall *call);
BASHKIT_API BashkitStatus bashkit_create_with_runtime_handler_v1(
    BashkitBytes config_json, BashkitRuntimeHandlerV1 handler, void *user_data,
    Bashkit **out_bash, BashkitError **out_error);
BASHKIT_API BashkitBytes bashkit_runtime_call_request(const BashkitRuntimeCall *call);
BASHKIT_API uint32_t bashkit_runtime_call_cancelled(const BashkitRuntimeCall *call);
BASHKIT_API BashkitStatus bashkit_runtime_call_respond_json(
    BashkitRuntimeCall *call, BashkitBytes value);
BASHKIT_API BashkitStatus bashkit_runtime_call_fail(BashkitRuntimeCall *call);
BASHKIT_API void bashkit_runtime_call_free(BashkitRuntimeCall *call);

/* Live byte delivery. The callback borrows bytes only for its duration and
   returns 0 after consuming the whole chunk, nonzero to fail execution.
   It runs on the thread calling bashkit_execute_streaming. It may call
   bashkit_cancel, but must not free the instance or call blocking APIs on any
   handle from the callback.
   context and callback must remain valid until this function returns.
   capture_limit is per stream; 0 disables capture. NULL callback discards output. */
#define BASHKIT_STREAM_STDOUT 1u
#define BASHKIT_STREAM_STDERR 2u
typedef uint32_t (*BashkitOutputCallback)(void *context, uint32_t stream, BashkitBytes bytes);
BASHKIT_API BashkitStatus bashkit_execute_streaming(
    Bashkit *bash, BashkitBytes script, size_t capture_limit,
    BashkitOutputCallback callback, void *context,
    BashkitResult **out_result, BashkitError **out_error);

/* Cancellation requires the `cancellation` capability. Both are lock-free and
   safe to call from any thread while bashkit_execute is blocked on the same
   handle; execution aborts at the next command boundary with
   BASHKIT_CANCELLED. The flag is sticky until bashkit_clear_cancel. */
BASHKIT_API BashkitStatus bashkit_cancel(Bashkit *bash);
BASHKIT_API BashkitStatus bashkit_clear_cancel(Bashkit *bash);

/* Result byte views remain valid until bashkit_result_free(result). */
BASHKIT_API int32_t bashkit_result_exit_code(const BashkitResult *result);
BASHKIT_API BashkitBytes bashkit_result_stdout(const BashkitResult *result);
BASHKIT_API BashkitBytes bashkit_result_stderr(const BashkitResult *result);
BASHKIT_API uint32_t bashkit_result_flags(const BashkitResult *result);
BASHKIT_API BashkitBytes bashkit_result_final_env_json(
    const BashkitResult *result);
BASHKIT_API void bashkit_result_free(BashkitResult *result);

BASHKIT_API BashkitStatus bashkit_write_file(
    Bashkit *bash,
    BashkitBytes path,
    BashkitBytes content,
    BashkitError **out_error);
BASHKIT_API BashkitStatus bashkit_read_file(
    Bashkit *bash,
    BashkitBytes path,
    BashkitBuffer **out_buffer,
    BashkitError **out_error);
BASHKIT_API BashkitStatus bashkit_mkdir(
    Bashkit *bash,
    BashkitBytes path,
    uint32_t recursive,
    BashkitError **out_error);
BASHKIT_API BashkitStatus bashkit_remove(
    Bashkit *bash,
    BashkitBytes path,
    uint32_t recursive,
    BashkitError **out_error);

/* Mounts require the `realfs-mounts` capability; host roots must resolve under
   an `allowed_mount_paths` prefix from the session config. Sensitive host
   paths (home trees, `/etc`, `.ssh`, ...) are refused unless the allowlist
   names the root exactly (TM-FS-013). */
BASHKIT_API BashkitStatus bashkit_mount(
    Bashkit *bash,
    BashkitBytes vfs_path,
    BashkitBytes host_root,
    uint32_t writable,
    BashkitError **out_error);
BASHKIT_API BashkitStatus bashkit_unmount(
    Bashkit *bash,
    BashkitBytes vfs_path,
    BashkitError **out_error);

/* Buffer byte views remain valid until bashkit_buffer_free(buffer). */
BASHKIT_API BashkitBytes bashkit_buffer_bytes(const BashkitBuffer *buffer);
BASHKIT_API void bashkit_buffer_free(BashkitBuffer *buffer);

/* Error message views remain valid until bashkit_error_free(error). */
BASHKIT_API uint32_t bashkit_error_code(const BashkitError *error);
BASHKIT_API BashkitBytes bashkit_error_message(const BashkitError *error);
BASHKIT_API void bashkit_error_free(BashkitError *error);

#ifdef __cplusplus
}
#endif

#endif
