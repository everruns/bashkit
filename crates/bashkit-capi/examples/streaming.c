/* Live output is borrowed only for the duration of this callback. */
#include "bashkit.h"
#include <assert.h>
#include <string.h>

struct Sink { size_t stdout_bytes; size_t stderr_bytes; };
static uint32_t output(void *context, uint32_t stream, BashkitBytes bytes) {
    struct Sink *sink = (struct Sink *)context;
    if (stream == BASHKIT_STREAM_STDOUT) sink->stdout_bytes += bytes.len;
    else if (stream == BASHKIT_STREAM_STDERR) sink->stderr_bytes += bytes.len;
    else return 1;
    return 0;
}

int main(void) {
    Bashkit *bash = NULL;
    BashkitError *error = NULL;
    BashkitResult *result = NULL;
    struct Sink sink = {0, 0};
    assert(bashkit_create_default(&bash, &error) == BASHKIT_OK);
    const char script[] = "printf 'hello'; printf 'error' >&2";
    BashkitBytes input = {(const uint8_t *)script, sizeof(script) - 1};
    assert(bashkit_execute_streaming(bash, input, 3, output, &sink,
                                    &result, &error) == BASHKIT_OK);
    assert(sink.stdout_bytes == 5 && sink.stderr_bytes == 5);
    BashkitBytes captured = bashkit_result_stdout(result);
    assert(captured.len == 3 && memcmp(captured.ptr, "hel", 3) == 0);
    assert(bashkit_result_flags(result) & BASHKIT_RESULT_STDOUT_TRUNCATED);
    bashkit_result_free(result);
    bashkit_free(bash);
    return 0;
}
