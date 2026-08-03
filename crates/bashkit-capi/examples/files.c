#include "bashkit.h"

#include <stdio.h>
#include <string.h>

static BashkitBytes bytes(const void *ptr, size_t len) {
    BashkitBytes value = {(const uint8_t *)ptr, len};
    return value;
}

static BashkitBytes text(const char *value) {
    return bytes(value, strlen(value));
}

static int check(BashkitStatus status, BashkitError *error) {
    if (status == BASHKIT_OK) {
        return 0;
    }
    BashkitBytes message = bashkit_error_message(error);
    fprintf(stderr, "bashkit error %u: ", (unsigned)status);
    fwrite(message.ptr, 1, message.len, stderr);
    fputc('\n', stderr);
    bashkit_error_free(error);
    return 1;
}

int main(void) {
    const char config[] =
        "{\"schema_version\":1,\"cwd\":\"/workspace\","
        "\"env\":{\"FORMAT\":\"upper\"}}";
    Bashkit *bash = NULL;
    BashkitError *error = NULL;
    BashkitStatus status = bashkit_create_json(
        bytes(config, sizeof(config) - 1), &bash, &error);
    if (check(status, error)) {
        return 1;
    }

    status = bashkit_mkdir(bash, text("/workspace"), 0, &error);
    if (check(status, error)) {
        bashkit_free(bash);
        return 1;
    }
    status = bashkit_write_file(
        bash, text("/workspace/input.txt"), text("hello c abi\n"), &error);
    if (check(status, error)) {
        bashkit_free(bash);
        return 1;
    }

    BashkitResult *result = NULL;
    const char script[] = "tr '[:lower:]' '[:upper:]' < input.txt > output.txt";
    status = bashkit_execute(
        bash, bytes(script, sizeof(script) - 1), &result, &error);
    if (check(status, error)) {
        bashkit_free(bash);
        return 1;
    }
    bashkit_result_free(result);

    BashkitBuffer *output = NULL;
    status = bashkit_read_file(
        bash, text("/workspace/output.txt"), &output, &error);
    if (check(status, error)) {
        bashkit_free(bash);
        return 1;
    }
    BashkitBytes content = bashkit_buffer_bytes(output);
    fwrite(content.ptr, 1, content.len, stdout);

    bashkit_buffer_free(output);
    bashkit_free(bash);
    return 0;
}
