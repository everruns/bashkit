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

static PyMethodDef bk_methods[] = {
    {"argv", bk_argv, METH_NOARGS, "WASI argv for this call."},
    {"environ", bk_environ, METH_NOARGS, "WASI environment for this call."},
    {"load_environ", bk_load_environ, METH_NOARGS, "Install this call's environment."},
    {"immortalize", bk_immortalize, METH_O, "Make snapshot objects immortal."},
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
