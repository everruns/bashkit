#include "bashkit.h"

#include <stdio.h>
#include <string.h>

static BashkitBytes bytes(const void *ptr, size_t len) {
    BashkitBytes value = {(const uint8_t *)ptr, len};
    return value;
}

static int report_error(BashkitStatus status, BashkitError *error) {
    BashkitBytes message = bashkit_error_message(error);
    fprintf(stderr, "bashkit error %u: ", (unsigned)status);
    fwrite(message.ptr, 1, message.len, stderr);
    fputc('\n', stderr);
    bashkit_error_free(error);
    return 1;
}

int main(void) {
    Bashkit *bash = NULL;
    BashkitError *error = NULL;
    BashkitStatus status = bashkit_create_default(&bash, &error);
    if (status != BASHKIT_OK) {
        return report_error(status, error);
    }

    const char script[] =
        "printf 'Hello from Bashkit! User: %s Time: ' \"$(whoami)\"; "
        "date -u '+%Y-%m-%dT%H:%M:%SZ'";
    BashkitResult *result = NULL;
    status = bashkit_execute(
        bash,
        bytes(script, strlen(script)),
        &result,
        &error);
    if (status != BASHKIT_OK) {
        bashkit_free(bash);
        return report_error(status, error);
    }

    BashkitBytes output = bashkit_result_stdout(result);
    fwrite(output.ptr, 1, output.len, stdout);
    int exit_code = bashkit_result_exit_code(result);

    bashkit_result_free(result);
    bashkit_free(bash);
    return exit_code;
}
