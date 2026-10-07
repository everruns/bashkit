// Bashkit CPython guest entrypoint (wasm32-wasip1, reactor model).
//
// Decisions:
// - Interpreter init runs once, at build time, inside `wizer-initialize`
//   (`wasmtime wizer`). The snapshot carries an initialized interpreter with
//   common stdlib modules imported, so every `python3` call starts from that
//   state copy-on-write instead of paying CPython startup (tens of millions of
//   instructions; ~460 ms on Pulley).
// - Per-call inputs (argv, environment, cwd, stdin) only exist at run time, so
//   `bashkit_run` reads them through WASI and hands them to the pure-Python
//   CLI driver in `_bashkit_boot.py`, which emulates CPython's command line.
// - One instance serves exactly one `python3` call. Nothing a tenant does can
//   leak into the next call because the host discards the instance.

#include <Python.h>
#define Py_BUILD_CORE 1
#include "internal/pycore_object.h"
#undef Py_BUILD_CORE
#include <stdlib.h>
#include <string.h>
#include <wasi/api.h>

extern void __wasm_call_ctors(void);

static int bk_initialized = 0;

// _bashkit.argv() -> list[str]: WASI argv for this call (argv[0] included).
static PyObject *bk_argv(PyObject *self, PyObject *unused) {
    (void)self;
    (void)unused;
    __wasi_size_t argc = 0, buf_size = 0;
    if (__wasi_args_sizes_get(&argc, &buf_size) != 0) {
        return PyErr_Format(PyExc_OSError, "args_sizes_get failed");
    }
    char **argv = calloc(argc + 1, sizeof(char *));
    char *buf = malloc(buf_size + 1);
    if (argv == NULL || buf == NULL) {
        free(argv);
        free(buf);
        return PyErr_NoMemory();
    }
    PyObject *list = NULL;
    if (__wasi_args_get((uint8_t **)argv, (uint8_t *)buf) != 0) {
        PyErr_Format(PyExc_OSError, "args_get failed");
        goto done;
    }
    list = PyList_New(0);
    if (list == NULL) {
        goto done;
    }
    for (__wasi_size_t i = 0; i < argc; i++) {
        PyObject *s = PyUnicode_DecodeFSDefault(argv[i]);
        if (s == NULL || PyList_Append(list, s) < 0) {
            Py_XDECREF(s);
            Py_CLEAR(list);
            goto done;
        }
        Py_DECREF(s);
    }
done:
    free(argv);
    free(buf);
    return list;
}

// _bashkit.environ() -> list[str]: WASI environment as "KEY=VALUE" strings.
static PyObject *bk_environ(PyObject *self, PyObject *unused) {
    (void)self;
    (void)unused;
    __wasi_size_t count = 0, buf_size = 0;
    if (__wasi_environ_sizes_get(&count, &buf_size) != 0) {
        return PyErr_Format(PyExc_OSError, "environ_sizes_get failed");
    }
    char **env = calloc(count + 1, sizeof(char *));
    char *buf = malloc(buf_size + 1);
    if (env == NULL || buf == NULL) {
        free(env);
        free(buf);
        return PyErr_NoMemory();
    }
    PyObject *list = NULL;
    if (__wasi_environ_get((uint8_t **)env, (uint8_t *)buf) != 0) {
        PyErr_Format(PyExc_OSError, "environ_get failed");
        goto done;
    }
    list = PyList_New(0);
    if (list == NULL) {
        goto done;
    }
    for (__wasi_size_t i = 0; i < count; i++) {
        PyObject *s = PyUnicode_DecodeFSDefault(env[i]);
        if (s == NULL || PyList_Append(list, s) < 0) {
            Py_XDECREF(s);
            Py_CLEAR(list);
            goto done;
        }
        Py_DECREF(s);
    }
done:
    free(env);
    free(buf);
    return list;
}

// _bashkit.load_environ() -> dict[bytes, bytes]: replace the C environment
// with this call's WASI environment and return it in `os.environ._data` form.
// Doing this in C instead of `os.environ.clear()` + per-key `__setitem__`
// (each one a putenv) cuts most of the driver's per-call Python work.
static PyObject *bk_load_environ(PyObject *self, PyObject *unused) {
    (void)self;
    (void)unused;
    __wasi_size_t count = 0, buf_size = 0;
    if (__wasi_environ_sizes_get(&count, &buf_size) != 0) {
        return PyErr_Format(PyExc_OSError, "environ_sizes_get failed");
    }
    char **env = calloc(count + 1, sizeof(char *));
    char *buf = malloc(buf_size + 1);
    if (env == NULL || buf == NULL) {
        free(env);
        free(buf);
        return PyErr_NoMemory();
    }
    PyObject *dict = NULL;
    if (__wasi_environ_get((uint8_t **)env, (uint8_t *)buf) != 0) {
        PyErr_Format(PyExc_OSError, "environ_get failed");
        goto done;
    }
    clearenv();
    dict = PyDict_New();
    if (dict == NULL) {
        goto done;
    }
    for (__wasi_size_t i = 0; i < count; i++) {
        char *eq = strchr(env[i], '=');
        if (eq == NULL || eq == env[i]) {
            continue;
        }
        *eq = '\0';
        if (setenv(env[i], eq + 1, 1) != 0) {
            PyErr_NoMemory();
            Py_CLEAR(dict);
            goto done;
        }
        PyObject *k = PyBytes_FromString(env[i]);
        PyObject *v = PyBytes_FromString(eq + 1);
        if (k == NULL || v == NULL || PyDict_SetItem(dict, k, v) < 0) {
            Py_XDECREF(k);
            Py_XDECREF(v);
            Py_CLEAR(dict);
            goto done;
        }
        Py_DECREF(k);
        Py_DECREF(v);
    }
done:
    free(env);
    free(buf);
    return dict;
}

// _bashkit.immortalize(objects) -> None: snapshot-time only. Immortal
// objects skip reference count writes, so a call that merely uses a
// preloaded module no longer dirties (and copy-on-write faults) the pages
// holding it. Immortal objects are never freed, which is the point: the
// snapshot heap outlives every call.
static PyObject *bk_immortalize(PyObject *self, PyObject *objects) {
    (void)self;
    PyObject *seq = PySequence_Fast(objects, "immortalize() expects a sequence");
    if (seq == NULL) {
        return NULL;
    }
    Py_ssize_t n = PySequence_Fast_GET_SIZE(seq);
    PyObject **items = PySequence_Fast_ITEMS(seq);
    for (Py_ssize_t i = 0; i < n; i++) {
        _Py_SetImmortal(items[i]);
    }
    Py_DECREF(seq);
    Py_RETURN_NONE;
}

// --- HTTP bridge ---------------------------------------------------------
// The host import runs the request through bashkit's HttpClient (allowlist,
// SSRF checks, credential injection, signing, transport, size caps). The
// guest never opens a socket and never sees TLS. Records are u32
// little-endian length-prefixed fields; see `cpython/http.rs` on the host.

__attribute__((import_module("bashkit"), import_name("http_request")))
int32_t bk_host_http_request(const uint8_t *req, int32_t len, int64_t timeout_ms,
                             uint32_t *out_len);
__attribute__((import_module("bashkit"), import_name("http_take")))
int32_t bk_host_http_take(uint8_t *buf, int32_t len);

enum { BK_HTTP_OK = 0, BK_HTTP_NETWORK = 1, BK_HTTP_TIMEOUT = 2, BK_HTTP_INVALID = 3 };

typedef struct {
    uint8_t *data;
    size_t len, cap;
} bk_buf;

static int bk_buf_put(bk_buf *b, const void *src, size_t n) {
    if (b->len + n > b->cap) {
        size_t cap = b->cap ? b->cap * 2 : 256;
        while (cap < b->len + n) {
            cap *= 2;
        }
        uint8_t *data = realloc(b->data, cap);
        if (data == NULL) {
            return -1;
        }
        b->data = data;
        b->cap = cap;
    }
    memcpy(b->data + b->len, src, n);
    b->len += n;
    return 0;
}

static int bk_buf_u32(bk_buf *b, uint32_t v) {
    uint8_t le[4] = {v & 0xff, (v >> 8) & 0xff, (v >> 16) & 0xff, (v >> 24) & 0xff};
    return bk_buf_put(b, le, 4);
}

static int bk_buf_field(bk_buf *b, const char *src, size_t n) {
    if (n > UINT32_MAX) {
        return -1;
    }
    return (bk_buf_u32(b, (uint32_t)n) < 0 || bk_buf_put(b, src, n) < 0) ? -1 : 0;
}

static int bk_buf_str(bk_buf *b, PyObject *s) {
    Py_ssize_t n;
    const char *utf8 = PyUnicode_AsUTF8AndSize(s, &n);
    if (utf8 == NULL) {
        return -1;
    }
    if (bk_buf_field(b, utf8, (size_t)n) < 0) {
        PyErr_NoMemory();
        return -1;
    }
    return 0;
}

typedef struct {
    const uint8_t *p, *end;
} bk_cursor;

static int bk_take_u32(bk_cursor *c, uint32_t *v) {
    if (c->end - c->p < 4) {
        return -1;
    }
    *v = (uint32_t)c->p[0] | ((uint32_t)c->p[1] << 8) | ((uint32_t)c->p[2] << 16) |
         ((uint32_t)c->p[3] << 24);
    c->p += 4;
    return 0;
}

static int bk_take_field(bk_cursor *c, const uint8_t **ptr, uint32_t *n) {
    if (bk_take_u32(c, n) < 0 || (size_t)(c->end - c->p) < *n) {
        return -1;
    }
    *ptr = c->p;
    c->p += *n;
    return 0;
}

// Decode `status, headers, body` into (int, list[tuple[str, str]], bytes).
static PyObject *bk_decode_response(const uint8_t *data, size_t len) {
    bk_cursor c = {data, data + len};
    uint32_t status, count;
    if (bk_take_u32(&c, &status) < 0 || bk_take_u32(&c, &count) < 0) {
        goto malformed;
    }
    PyObject *headers = PyList_New(0);
    if (headers == NULL) {
        return NULL;
    }
    for (uint32_t i = 0; i < count; i++) {
        const uint8_t *k, *v;
        uint32_t kn, vn;
        if (bk_take_field(&c, &k, &kn) < 0 || bk_take_field(&c, &v, &vn) < 0) {
            Py_DECREF(headers);
            goto malformed;
        }
        PyObject *pair = Py_BuildValue("(s#s#)", (const char *)k, (Py_ssize_t)kn,
                                       (const char *)v, (Py_ssize_t)vn);
        if (pair == NULL || PyList_Append(headers, pair) < 0) {
            Py_XDECREF(pair);
            Py_DECREF(headers);
            return NULL;
        }
        Py_DECREF(pair);
    }
    const uint8_t *body;
    uint32_t body_len;
    if (bk_take_field(&c, &body, &body_len) < 0) {
        Py_DECREF(headers);
        goto malformed;
    }
    return Py_BuildValue("(INy#)", (unsigned int)status, headers, (const char *)body,
                         (Py_ssize_t)body_len);
malformed:
    PyErr_SetString(PyExc_OSError, "malformed HTTP response from host");
    return NULL;
}

// _bashkit.http(method, url, headers, body, timeout) -> (status, headers, body)
// headers: iterable of (name, value) str pairs; body: bytes-like or None;
// timeout: seconds (float) or None. Raises ConnectionError, TimeoutError or
// ValueError (request rejected by the host).
static PyObject *bk_http(PyObject *self, PyObject *args) {
    (void)self;
    PyObject *method, *url, *headers, *body_obj, *timeout_obj;
    if (!PyArg_ParseTuple(args, "UUOOO:http", &method, &url, &headers, &body_obj,
                          &timeout_obj)) {
        return NULL;
    }
    int64_t timeout_ms = -1;
    if (timeout_obj != Py_None) {
        double t = PyFloat_AsDouble(timeout_obj);
        if (t == -1.0 && PyErr_Occurred()) {
            return NULL;
        }
        timeout_ms = t <= 0 ? 0 : (t > 9.0e15 ? INT64_MAX : (int64_t)(t * 1000.0));
    }
    Py_buffer body = {0};
    if (body_obj != Py_None && PyObject_GetBuffer(body_obj, &body, PyBUF_SIMPLE) < 0) {
        return NULL;
    }
    PyObject *result = NULL;
    PyObject *seq = NULL;
    uint8_t *resp = NULL;
    bk_buf req = {0};
    if (bk_buf_str(&req, method) < 0 || bk_buf_str(&req, url) < 0) {
        goto done;
    }
    seq = PySequence_Fast(headers, "headers must be a sequence of (name, value) pairs");
    if (seq == NULL) {
        goto done;
    }
    Py_ssize_t n = PySequence_Fast_GET_SIZE(seq);
    if (n > UINT32_MAX || bk_buf_u32(&req, (uint32_t)n) < 0) {
        PyErr_NoMemory();
        goto done;
    }
    for (Py_ssize_t i = 0; i < n; i++) {
        PyObject *pair = PySequence_Fast_GET_ITEM(seq, i);
        if (!PyTuple_Check(pair) || PyTuple_GET_SIZE(pair) != 2 ||
            !PyUnicode_Check(PyTuple_GET_ITEM(pair, 0)) ||
            !PyUnicode_Check(PyTuple_GET_ITEM(pair, 1))) {
            PyErr_SetString(PyExc_TypeError, "headers must be (str, str) pairs");
            goto done;
        }
        if (bk_buf_str(&req, PyTuple_GET_ITEM(pair, 0)) < 0 ||
            bk_buf_str(&req, PyTuple_GET_ITEM(pair, 1)) < 0) {
            goto done;
        }
    }
    if (bk_buf_field(&req, body.buf ? body.buf : "", (size_t)body.len) < 0 ||
        req.len > INT32_MAX) {
        PyErr_SetString(PyExc_ValueError, "request too large");
        goto done;
    }
    uint32_t resp_len = 0;
    int32_t code;
    code = bk_host_http_request(req.data, (int32_t)req.len, timeout_ms, &resp_len);
    if (code > BK_HTTP_INVALID) {
        PyErr_SetString(PyExc_OSError, "HTTP bridge fault");
        goto done;
    }
    resp = malloc(resp_len ? resp_len : 1);
    if (resp == NULL) {
        // Drop the pending reply so the next request starts clean.
        bk_host_http_take(NULL, -1);
        PyErr_NoMemory();
        goto done;
    }
    if (bk_host_http_take(resp, (int32_t)resp_len) != BK_HTTP_OK) {
        PyErr_SetString(PyExc_OSError, "HTTP bridge fault");
        goto done;
    }
    if (code == BK_HTTP_OK) {
        result = bk_decode_response(resp, resp_len);
    } else {
        PyObject *kind = code == BK_HTTP_TIMEOUT   ? PyExc_TimeoutError
                         : code == BK_HTTP_INVALID ? PyExc_ValueError
                                                   : PyExc_ConnectionError;
        PyObject *msg = PyUnicode_DecodeUTF8((const char *)resp, resp_len, "replace");
        if (msg != NULL) {
            PyErr_SetObject(kind, msg);
            Py_DECREF(msg);
        }
    }
done:
    free(resp);
    free(req.data);
    Py_XDECREF(seq);
    if (body.obj != NULL) {
        PyBuffer_Release(&body);
    }
    return result;
}

static PyMethodDef bk_methods[] = {
    {"argv", bk_argv, METH_NOARGS, "WASI argv for this call."},
    {"environ", bk_environ, METH_NOARGS, "WASI environment for this call."},
    {"load_environ", bk_load_environ, METH_NOARGS, "Install this call's environment."},
    {"immortalize", bk_immortalize, METH_O, "Make snapshot objects immortal."},
    {"http", bk_http, METH_VARARGS, "Send one HTTP request through the host."},
    {NULL, NULL, 0, NULL},
};

static struct PyModuleDef bk_module = {
    PyModuleDef_HEAD_INIT, "_bashkit", NULL, -1, bk_methods,
};

static PyObject *bk_module_init(void) { return PyModule_Create(&bk_module); }

static void bk_fatal(PyStatus status) {
    // Init failures are build-time bugs; abort loudly so wizer fails.
    if (PyStatus_Exception(status)) {
        Py_ExitStatusException(status);
    }
}

static void bk_initialize(void) {
    if (bk_initialized) {
        return;
    }
    __wasm_call_ctors();

    PyPreConfig preconfig;
    PyPreConfig_InitIsolatedConfig(&preconfig);
    preconfig.utf8_mode = 1;
    bk_fatal(Py_PreInitialize(&preconfig));

    if (PyImport_AppendInittab("_bashkit", bk_module_init) < 0) {
        abort();
    }

    PyConfig config;
    PyConfig_InitIsolatedConfig(&config);
    config.site_import = 1;
    config.write_bytecode = 0;
    config.install_signal_handlers = 0;
    config.buffered_stdio = 1;
    config.user_site_directory = 0;
    config.pathconfig_warnings = 0;
    bk_fatal(PyConfig_SetString(&config, &config.home, L"/usr/local"));
    bk_fatal(PyConfig_SetString(&config, &config.program_name, L"python3"));
    bk_fatal(Py_InitializeFromConfig(&config));
    PyConfig_Clear(&config);

    if (PyRun_SimpleString("import _bashkit_boot\n_bashkit_boot.preload()\n") != 0) {
        abort();
    }
    bk_initialized = 1;
}

__attribute__((export_name("wizer-initialize"))) void bk_wizer_initialize(void) {
    bk_initialize();
}

// Run one `python3` invocation; returns the process exit status.
__attribute__((export_name("bashkit_run"))) int bk_run(void) {
    bk_initialize();
    PyObject *boot = PyImport_ImportModule("_bashkit_boot");
    if (boot == NULL) {
        PyErr_Print();
        return 1;
    }
    PyObject *result = PyObject_CallMethod(boot, "main", NULL);
    Py_DECREF(boot);
    if (result == NULL) {
        // main() handles SystemExit and tracebacks itself; reaching here is
        // an internal error in the driver.
        PyErr_Print();
        return 1;
    }
    long code = PyLong_AsLong(result);
    Py_DECREF(result);
    if (code == -1 && PyErr_Occurred()) {
        PyErr_Clear();
        code = 1;
    }
    return (int)code;
}
