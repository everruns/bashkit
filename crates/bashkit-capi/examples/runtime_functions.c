#include "bashkit.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

static BashkitBytes bytes(const char *text) {
    BashkitBytes value = {(const uint8_t *)text, strlen(text)};
    return value;
}

static void lookup(void *data, BashkitRuntimeCall *call) {
    unsigned *calls = (unsigned *)data;
    BashkitBytes request = bashkit_runtime_call_request(call);
    assert(request.len > 0);
    assert(!bashkit_runtime_call_cancelled(call));
    ++*calls;
    /* A real dispatcher parses request JSON and calls an application service.
       It may instead transfer call to a worker, respond there, then free it. */
    assert(bashkit_runtime_call_respond_json(call, bytes("42")) == BASHKIT_OK);
    bashkit_runtime_call_free(call);
}

int main(void) {
    const char *config = "{\"schema_version\":1,\"python\":true,\"typescript\":true,"
        "\"external_functions\":[\"lookup\"],\"typescript_external_functions\":[\"lookup\"]}";
    unsigned calls = 0;
    Bashkit *bash = NULL;
    BashkitError *error = NULL;
    assert(bashkit_create_with_runtime_handler_v1(bytes(config), lookup, &calls, &bash, &error) == BASHKIT_OK);
    BashkitResult *result = NULL;
    assert(bashkit_execute(bash, bytes("python -c 'print(lookup())'; ts -c 'await lookup()'"), &result, &error) == BASHKIT_OK);
    assert(bashkit_result_exit_code(result) == 0);
    BashkitBytes output = bashkit_result_stdout(result);
    assert(output.len == 6 && memcmp(output.ptr, "42\n42\n", 6) == 0);
    assert(calls == 2);
    fwrite(output.ptr, 1, output.len, stdout);
    bashkit_result_free(result);
    bashkit_free(bash);
    return 0;
}
