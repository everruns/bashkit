"""Bashkit CPython guest driver.

Runs inside the wasm32-wasip1 CPython snapshot. `preload()` executes once at
build time (wizer); `main()` executes once per `python3` call and emulates
CPython's command line (`-c`, `-m`, script, `-`, stdin) on an interpreter that
is already initialized.

Decisions:
- Tracebacks hide this driver's frames so output matches real `python3`.
- `random` is re-seeded per call: the snapshot would otherwise hand every
  tenant the same sequence. Seeding is lazy (first use in a call), since it
  costs ~0.2 ms on Pulley and most calls never touch `random`. Calling the
  C base method directly (`_random.Random.random(random._inst)`) skips the
  seed and sees the snapshot's state; `random` is not a CSPRNG anyway.
- The snapshot heap is frozen (`gc.freeze`) so garbage collection only
  scans objects created by the current call.
- Snapshot objects are made immortal before freezing: reference counting
  then never writes to them, so a call that only uses preloaded modules
  leaves their pages shared instead of copy-on-write faulting them in
  (~20% fewer faults per call). They are never freed, which is fine: the
  snapshot outlives every call.
- The per-call environment is installed from C (`_bashkit.load_environ`);
  per-key `os.environ` updates were measurable driver overhead on Pulley.
- asyncio's selector loop normally wakes itself through a socketpair. WASI
  has no sockets and the guest has no threads or signals that could need a
  wakeup, so the self-pipe is disabled (patched once, in the snapshot).
- Options that only matter at interpreter init (-E, -I, -s, -S, -B, -u, -O,
  -q, -b, -d, -v, -R, -P, -X) are accepted and ignored; the snapshot already
  runs isolated, unbuffered output is irrelevant because the host captures
  stdout, and bytecode is never written.
"""

import sys

_PRELOAD = (
    "abc", "argparse", "ast", "base64", "binascii", "bisect", "calendar",
    "collections", "collections.abc", "contextlib", "copy", "csv", "dataclasses",
    "datetime", "decimal", "difflib", "enum", "fnmatch", "fractions",
    "functools", "glob", "gzip", "hashlib", "heapq", "html", "io", "itertools",
    "json", "keyword", "linecache", "locale", "logging", "math", "numbers",
    "operator", "os", "pathlib", "pprint", "random", "re", "shlex", "shutil",
    "statistics", "string", "struct", "subprocess", "tempfile", "textwrap",
    "time", "token", "tokenize", "traceback", "types", "typing", "unicodedata",
    "urllib.parse", "uuid", "warnings", "weakref", "zlib", "runpy",
    "_bashkit", "atexit", "builtins", "importlib", "sqlite3", "zipfile",
    "asyncio",
)

_USAGE = """\
usage: python3 [option] ... [-c cmd | -m mod | file | -] [arg] ...
Options (and corresponding environment variables):
-c cmd : program passed in as string (terminates option list)
-h     : print this help message and exit (also -? or --help)
-m mod : run library module as a script (terminates option list)
-V     : print the Python version number and exit (also --version)
         when given twice, print more information about the build
-W arg : warning control
-x     : skip first line of source
file   : program read from script file
-      : program read from stdin (default; interactive mode if a tty)
arg ...: arguments passed to program in sys.argv[1:]

Accepted for compatibility and ignored: -b -B -d -E -i -I -O -OO -P -q -R -s -S -u -v -X opt
"""

_NOARG_FLAGS = set("bBdEiIOPqRsSuvx")


def preload():
    """Import common stdlib modules into the snapshot."""
    import importlib

    for name in _PRELOAD:
        try:
            importlib.import_module(name)
        except Exception:  # pragma: no cover - optional modules
            pass
    _patch_asyncio()
    _install_lazy_random()
    # Move every object the snapshot holds into the permanent generation.
    # The collector then never traverses (or writes to) snapshot memory, so
    # per-call GC cost scales with what the script allocates, and snapshot
    # pages stay shared copy-on-write instead of being dirtied by GC headers.
    import gc

    gc.collect()
    _immortalize_snapshot()
    gc.freeze()


def _immortalize_snapshot():
    # Immortal objects skip reference count writes, so a call that only uses
    # preloaded modules leaves their pages shared instead of copy-on-write
    # faulting them in. Covers every GC-tracked object plus what they refer
    # to directly (strings, ints, bytes are not tracked themselves).
    import gc

    import _bashkit

    objects = gc.get_objects(generation=None)
    _bashkit.immortalize(objects)
    _bashkit.immortalize(gc.get_referents(*objects))


_RANDOM_API = (
    "seed", "random", "uniform", "triangular", "randint", "choice", "randrange",
    "sample", "shuffle", "choices", "normalvariate", "lognormvariate",
    "expovariate", "vonmisesvariate", "gammavariate", "gauss", "betavariate",
    "binomialvariate", "paretovariate", "weibullvariate", "getstate",
    "setstate", "getrandbits", "randbytes",
)
_LAZY_ENTRY_POINTS = ("random", "getrandbits", "getstate", "seed", "setstate")


def _install_lazy_random():
    # The module-level functions are methods bound to `random._inst`. Give
    # that instance a subclass whose entry points seed it from os.urandom on
    # first use, then turn it back into a plain Random and rebind the module
    # functions, so later calls pay nothing extra.
    import random

    base = random.Random

    def wake(self):
        # Only the overridden entry points need rebinding; the other module
        # functions are base-class methods and see the class switch. Module
        # attribute writes are slow on Pulley (~40 us each), so keep it to five.
        self.__class__ = base
        for name in _LAZY_ENTRY_POINTS:
            setattr(random, name, getattr(self, name))

    class _LazyRandom(base):
        def random(self):
            wake(self)
            base.seed(self)
            return self.random()

        def getrandbits(self, k):
            wake(self)
            base.seed(self)
            return self.getrandbits(k)

        def getstate(self):
            wake(self)
            base.seed(self)
            return self.getstate()

        def seed(self, *args, **kwargs):
            wake(self)
            return self.seed(*args, **kwargs)

        def setstate(self, state):
            wake(self)
            return self.setstate(state)

    inst = random._inst
    inst.__class__ = _LazyRandom
    for name in _RANDOM_API:
        setattr(random, name, getattr(inst, name))


def _patch_asyncio():
    try:
        from asyncio import selector_events
    except Exception:  # pragma: no cover
        return
    loop = selector_events.BaseSelectorEventLoop
    loop._make_self_pipe = lambda self: None
    loop._close_self_pipe = lambda self: None
    loop._write_to_self = lambda self: None


class _UsageError(Exception):
    pass


def _parse(args):
    """Parse CPython-style options. Returns (mode, value, rest, flags)."""
    flags = {"x": False, "V": 0, "warnings": []}
    i = 0
    while i < len(args):
        arg = args[i]
        if arg == "--":
            i += 1
            break
        if arg in ("--version",):
            flags["V"] += 1
            i += 1
            continue
        if arg == "--help" or arg == "--help-all":
            return ("help", None, [], flags)
        if arg.startswith("--"):
            if arg == "--check-hash-based-pycs":
                i += 2
                continue
            raise _UsageError(f"unknown option {arg}")
        if not arg.startswith("-") or arg == "-":
            break
        j = 1
        while j < len(arg):
            ch = arg[j]
            if ch in ("c", "m"):
                value = arg[j + 1 :]
                rest_start = i + 1
                if not value:
                    if i + 1 >= len(args):
                        raise _UsageError(f"Argument expected for the -{ch} option")
                    value = args[i + 1]
                    rest_start = i + 2
                mode = "command" if ch == "c" else "module"
                return (mode, value, args[rest_start:], flags)
            if ch in ("W", "X"):
                value = arg[j + 1 :]
                if not value:
                    if i + 1 >= len(args):
                        raise _UsageError(f"Argument expected for the -{ch} option")
                    value = args[i + 1]
                    i += 1
                if ch == "W":
                    flags["warnings"].append(value)
                break
            if ch in ("h", "?"):
                return ("help", None, [], flags)
            if ch == "V":
                flags["V"] += 1
            elif ch == "x":
                flags["x"] = True
            elif ch not in _NOARG_FLAGS:
                raise _UsageError(f"Unknown option: -{ch}")
            j += 1
        i += 1
    if flags["V"]:
        return ("version", None, [], flags)
    if i < len(args):
        if args[i] == "-":
            return ("stdin", "-", args[i + 1 :], flags)
        return ("script", args[i], args[i + 1 :], flags)
    return ("stdin", "", [], flags)


def _refresh_process_state():
    import os

    import _bashkit

    # The C side replaces the process environment and hands back the
    # encoded mapping; per-key os.environ updates cost ~1 ms on Pulley.
    os.environ._data = _bashkit.load_environ()
    limit = os.environ.pop("__BASHKIT_RECURSION_LIMIT", None)
    if limit:
        try:
            sys.setrecursionlimit(max(50, int(limit)))
        except (ValueError, RecursionError):
            pass
    pwd = os.environ.get("PWD")
    if pwd:
        try:
            os.chdir(pwd)
        except OSError:
            pass


def _fresh_main(filename=None):
    import builtins
    import types

    module = types.ModuleType("__main__")
    module.__builtins__ = builtins
    if filename is not None:
        module.__file__ = filename
    module.__cached__ = None
    sys.modules["__main__"] = module
    return module


def _strip_driver_frames(tb):
    # The code's own filename, not __file__ (that names the .pyc).
    me = _strip_driver_frames.__code__.co_filename
    while tb is not None and tb.tb_frame.f_code.co_filename in (me, "<frozen runpy>"):
        tb = tb.tb_next
    return tb


def _report(exc):
    # The default excepthook formats `exc.__traceback__`, so strip the driver
    # frames on the exception itself, not only on the argument.
    tb = _strip_driver_frames(exc.__traceback__)
    exc = exc.with_traceback(tb)
    sys.excepthook(type(exc), exc, tb)


def _exit_code(exc):
    code = exc.code
    if code is None:
        return 0
    if isinstance(code, bool):
        return int(code)
    if isinstance(code, int):
        return code & 0xFF
    try:
        print(code, file=sys.stderr)
    except Exception:
        pass
    return 1


def _release_main():
    # Real CPython finalizes modules at exit, which closes files a script left
    # open and flushes their buffers. Drop __main__'s globals and collect so
    # those writes reach the filesystem before the host tears the instance down.
    try:
        import gc

        main = sys.modules.get("__main__")
        if main is not None:
            main.__dict__.clear()
        gc.collect()
    except BaseException:
        pass


def _flush():
    for stream in (sys.stdout, sys.stderr):
        try:
            if stream is not None:
                stream.flush()
        except Exception:
            pass


def _run_source(source, filename, module):
    code = compile(source, filename, "exec", dont_inherit=True)
    exec(code, module.__dict__)


def _execute(mode, value, rest, flags):
    import os

    if mode == "help":
        sys.stdout.write(_USAGE)
        return 0
    if mode == "version":
        if flags["V"] > 1:
            sys.stdout.write(f"Python {sys.version}\n")
        else:
            v = sys.version_info
            sys.stdout.write(f"Python {v.major}.{v.minor}.{v.micro}\n")
        return 0
    if flags["warnings"]:
        import warnings

        warnings._processoptions(flags["warnings"])

    if mode == "command":
        sys.argv = ["-c", *rest]
        sys.path.insert(0, "")
        _run_source(value, "<string>", _fresh_main())
        return 0
    if mode == "module":
        import runpy

        sys.argv = [value, *rest]
        sys.path.insert(0, os.getcwd())
        _fresh_main()
        runpy._run_module_as_main(value, alter_argv=True)
        return 0
    if mode == "stdin":
        sys.argv = [value, *rest]
        sys.path.insert(0, "")
        source = sys.stdin.read()
        _run_source(source, "<stdin>", _fresh_main())
        return 0
    # script
    path = value
    sys.argv = [path, *rest]
    if os.path.isdir(path):
        import runpy

        sys.path.insert(0, path)
        runpy.run_path(path, run_name="__main__")
        return 0
    try:
        with open(path, "rb") as f:
            source = f.read()
    except OSError as e:
        sys.stderr.write(
            f"python3: can't open file {path!r}: [Errno {e.errno}] {e.strerror}\n"
        )
        return 2
    if flags["x"]:
        nl = source.find(b"\n")
        source = b"" if nl < 0 else b"\n" + source[nl + 1 :]
    sys.path.insert(0, os.path.dirname(os.path.abspath(path)))
    _run_source(source, path, _fresh_main(path))
    return 0


def main():
    import _bashkit

    _refresh_process_state()
    args = _bashkit.argv()[1:]
    try:
        try:
            mode, value, rest, flags = _parse(args)
        except _UsageError as e:
            sys.stderr.write(f"{e}\n{_USAGE.splitlines()[0]}\n")
            sys.stderr.write("Try `python -h' for more information.\n")
            return 2
        try:
            status = _execute(mode, value, rest, flags)
        except SystemExit as e:
            status = _exit_code(e)
        except KeyboardInterrupt as e:
            _report(e)
            status = 130
        except BaseException as e:
            _report(e)
            status = 1
        try:
            import atexit

            atexit._run_exitfuncs()
        except SystemExit as e:
            status = _exit_code(e)
        except BaseException:
            pass
        _release_main()
        return status
    finally:
        _flush()
