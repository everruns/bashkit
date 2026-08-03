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
