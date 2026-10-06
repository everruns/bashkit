// Capability tests for the CPython (WASI) python3 builtin (`cpython` feature).
//
// One table of small programs covering the language and the stdlib modules
// scripts reach for. Each case runs through bashkit and must print exactly
// the expected output. `cases_match_host_python` runs the same table through
// the host's real `python3` (when one is installed) so the expectations stay
// honest; it skips cases that only make sense inside the sandbox.

#![cfg(feature = "cpython")]

use bashkit::Bash;

struct Case {
    name: &'static str,
    code: &'static str,
    expected: &'static str,
    /// Also compare against the host interpreter (version-stable output).
    host: bool,
}

const fn case(name: &'static str, code: &'static str, expected: &'static str) -> Case {
    Case {
        name,
        code,
        expected,
        host: true,
    }
}

const fn sandbox_only(name: &'static str, code: &'static str, expected: &'static str) -> Case {
    Case {
        name,
        code,
        expected,
        host: false,
    }
}

const CASES: &[Case] = &[
    // --- language ------------------------------------------------------------
    case(
        "classes_inheritance",
        "class A:\n    def hi(self): return 'A'\nclass B(A):\n    def hi(self): return 'B' + super().hi()\nprint(B().hi())",
        "BA\n",
    ),
    case(
        "dunder_methods",
        "class V:\n    def __init__(s, x): s.x = x\n    def __add__(s, o): return V(s.x + o.x)\n    def __repr__(s): return f'V({s.x})'\nprint(V(1) + V(2))",
        "V(3)\n",
    ),
    case(
        "properties_and_slots",
        "class P:\n    __slots__ = ('_v',)\n    def __init__(s): s._v = 1\n    @property\n    def v(s): return s._v * 10\nprint(P().v)",
        "10\n",
    ),
    case(
        "match_statement",
        "def f(x):\n    match x:\n        case {'k': [a, *rest]}: return a, rest\n        case _: return None\nprint(f({'k': [1, 2, 3]}))",
        "(1, [2, 3])\n",
    ),
    case(
        "generators_and_yield_from",
        "def g():\n    yield from range(3)\n    yield 9\nprint(list(g()))",
        "[0, 1, 2, 9]\n",
    ),
    case(
        "closures_nonlocal",
        "def c():\n    n = 0\n    def inc():\n        nonlocal n\n        n += 1\n        return n\n    return inc\ni = c(); i(); print(i())",
        "2\n",
    ),
    case(
        "decorators",
        "import functools\ndef twice(f):\n    @functools.wraps(f)\n    def w(*a): return f(*a) * 2\n    return w\n@twice\ndef sq(x): return x * x\nprint(sq(3), sq.__name__)",
        "18 sq\n",
    ),
    case(
        "exceptions_chaining",
        "try:\n    try:\n        {}['x']\n    except KeyError as e:\n        raise ValueError('wrapped') from e\nexcept ValueError as e:\n    print(type(e.__cause__).__name__, e)",
        "KeyError wrapped\n",
    ),
    case(
        "exception_groups",
        "try:\n    raise ExceptionGroup('g', [ValueError(1), TypeError(2)])\nexcept* ValueError as eg:\n    print('v', len(eg.exceptions))\nexcept* TypeError:\n    print('t')",
        "v 1\nt\n",
    ),
    case(
        "context_managers",
        "import contextlib\n@contextlib.contextmanager\ndef cm():\n    print('in'); yield 5; print('out')\nwith cm() as v: print(v)",
        "in\n5\nout\n",
    ),
    case(
        "comprehensions_walrus",
        "print({k: v for k, v in zip('ab', [1, 2])}, [y for x in range(5) if (y := x * x) > 4])",
        "{'a': 1, 'b': 2} [9, 16]\n",
    ),
    case(
        "fstrings",
        "x = 3.14159; n = 'w'\nprint(f'{x:.2f}|{n!r:>5}|{x=:.1f}|{1000000:,}')",
        "3.14|  'w'|x=3.1|1,000,000\n",
    ),
    case(
        "big_ints",
        "print(2 ** 100, (10 ** 30) // 7)",
        "1267650600228229401496703205376 142857142857142857142857142857\n",
    ),
    case(
        "float_repr",
        "print(0.1 + 0.2, 1e308 * 10, round(2.675, 2))",
        "0.30000000000000004 inf 2.67\n",
    ),
    case(
        "complex_numbers",
        "print((1 + 2j) * (3 - 1j), abs(3 + 4j))",
        "(5+5j) 5.0\n",
    ),
    case(
        "bytes_and_bytearray",
        "b = bytearray(b'abc'); b[0] = 65; print(bytes(b), b.hex(), bytes.fromhex('ff00'))",
        "b'Abc' 416263 b'\\xff\\x00'\n",
    ),
    case(
        "str_methods",
        "print('a,b,,c'.split(','), ' x '.strip(), 'abc'.title(), 'ß'.upper(), 'x'.center(5, '*'))",
        "['a', 'b', '', 'c'] x Abc SS **x**\n",
    ),
    case(
        "sorting_stable_key",
        "d = [('b', 2), ('a', 2), ('c', 1)]\nprint(sorted(d, key=lambda t: t[1]))",
        "[('c', 1), ('b', 2), ('a', 2)]\n",
    ),
    case(
        "dict_order_and_merge",
        "a = {'x': 1}; b = {'y': 2}\nprint(a | b, list({3: 0, 1: 0, 2: 0}))",
        "{'x': 1, 'y': 2} [3, 1, 2]\n",
    ),
    case(
        "sets",
        "print(sorted({1, 2, 3} & {2, 3, 4}), sorted({1} ^ {2}), frozenset() == set())",
        "[2, 3] [1, 2] True\n",
    ),
    case(
        "type_hints_runtime",
        "from typing import get_type_hints\ndef f(a: int, b: 'list[str]') -> None: ...\nprint(get_type_hints(f)['a'].__name__)",
        "int\n",
    ),
    case(
        "metaclass_and_init_subclass",
        "class R:\n    reg = []\n    def __init_subclass__(cls, **kw): R.reg.append(cls.__name__)\nclass X(R): pass\nclass Y(R): pass\nprint(R.reg)",
        "['X', 'Y']\n",
    ),
    case(
        "async_await",
        "import asyncio\nasync def w(n):\n    await asyncio.sleep(0)\n    return n * 2\nasync def main():\n    return await asyncio.gather(w(1), w(2), w(3))\nprint(asyncio.run(main()))",
        "[2, 4, 6]\n",
    ),
    case(
        "async_sleep_timers",
        "import asyncio\nasync def main():\n    out = []\n    async def t(n, d):\n        await asyncio.sleep(d); out.append(n)\n    await asyncio.gather(t('slow', 0.02), t('fast', 0.0))\n    return out\nprint(asyncio.run(main()))",
        "['fast', 'slow']\n",
    ),
    case(
        "async_queue_tasks",
        "import asyncio\nasync def main():\n    q = asyncio.Queue()\n    async def prod():\n        for i in range(3): await q.put(i)\n        await q.put(None)\n    async def cons():\n        s = 0\n        while (i := await q.get()) is not None: s += i\n        return s\n    _, s = await asyncio.gather(prod(), cons())\n    return s\nprint(asyncio.run(main()))",
        "3\n",
    ),
    case(
        "eval_exec_compile",
        "ns = {}\nexec(compile('y = 2 + 3', 'm', 'exec'), ns)\nprint(ns['y'], eval('y * 2', ns))",
        "5 10\n",
    ),
    case(
        "recursion_moderate",
        "def f(n): return 0 if n == 0 else 1 + f(n - 1)\nprint(f(500))",
        "500\n",
    ),
    // --- stdlib --------------------------------------------------------------
    case(
        "json",
        "import json\nd = json.loads('{\"a\": [1, 2.5, null, true]}')\nprint(d, json.dumps(d, sort_keys=True, indent=None))",
        "{'a': [1, 2.5, None, True]} {\"a\": [1, 2.5, null, true]}\n",
    ),
    case(
        "re",
        "import re\nprint(re.findall(r'(\\w)(\\d+)', 'a1 b22'), re.sub(r'(?P<w>\\w+)@', r'<\\g<w>>', 'me@x'))",
        "[('a', '1'), ('b', '22')] <me>x\n",
    ),
    case(
        "csv",
        "import csv, io\nr = list(csv.DictReader(io.StringIO('a,b\\n1,\"x,y\"\\n')))\nw = io.StringIO(); csv.writer(w).writerow(['q\"', 1])\nprint(r, w.getvalue().strip())",
        "[{'a': '1', 'b': 'x,y'}] \"q\"\"\",1\n",
    ),
    case(
        "collections",
        "from collections import Counter, defaultdict, deque, OrderedDict, namedtuple\nP = namedtuple('P', 'x y')\nd = deque([1, 2]); d.appendleft(0)\ndd = defaultdict(list); dd['k'].append(1)\nprint(Counter('abca').most_common(1), list(d), dict(dd), P(1, 2))",
        "[('a', 2)] [0, 1, 2] {'k': [1]} P(x=1, y=2)\n",
    ),
    case(
        "itertools",
        "import itertools as it\nprint(list(it.combinations('abc', 2)), list(it.accumulate([1, 2, 3])), list(it.islice(it.count(5), 3)), list(it.chain.from_iterable([[1], [2]])))",
        "[('a', 'b'), ('a', 'c'), ('b', 'c')] [1, 3, 6] [5, 6, 7] [1, 2]\n",
    ),
    case(
        "functools",
        "import functools\n@functools.lru_cache\ndef fib(n): return n if n < 2 else fib(n - 1) + fib(n - 2)\nprint(fib(80), functools.reduce(lambda a, b: a * b, range(1, 6)))",
        "23416728348467685 120\n",
    ),
    case(
        "dataclasses",
        "from dataclasses import dataclass, field, asdict\n@dataclass(order=True)\nclass P:\n    x: int\n    tags: list = field(default_factory=list)\nprint(asdict(P(1)), P(1) < P(2))",
        "{'x': 1, 'tags': []} True\n",
    ),
    case(
        "enum",
        "import enum\nclass C(enum.Enum):\n    RED = 1\n    BLUE = enum.auto()\nprint(C.BLUE, C(1).name, [c.value for c in C])",
        "C.BLUE RED [1, 2]\n",
    ),
    case(
        "datetime",
        "from datetime import datetime, timedelta, timezone, date\nd = datetime(2020, 1, 31, 12, tzinfo=timezone.utc)\nprint((d + timedelta(days=1)).isoformat(), date(2024, 2, 29).strftime('%a %d %b'), datetime.fromisoformat('2021-05-01T10:00:00').weekday())",
        "2020-02-01T12:00:00+00:00 Thu 29 Feb 5\n",
    ),
    case(
        "decimal_fractions",
        "from decimal import Decimal, getcontext\nfrom fractions import Fraction\ngetcontext().prec = 30\nprint(Decimal(1) / Decimal(7), Fraction(1, 3) + Fraction(1, 6), Decimal('0.1') + Decimal('0.2'))",
        "0.142857142857142857142857142857 1/2 0.3\n",
    ),
    case(
        "math_statistics",
        "import math, statistics\nprint(math.factorial(20), math.isqrt(10 ** 10), round(math.pi, 5), statistics.median([3, 1, 2]), statistics.mean([1, 2, 3, 4]))",
        "2432902008176640000 100000 3.14159 2 2.5\n",
    ),
    case(
        "hashlib",
        "import hashlib\nprint(hashlib.sha256(b'abc').hexdigest()[:16], hashlib.md5(b'x').hexdigest()[:8], hashlib.sha1(b'').hexdigest()[:8], hashlib.blake2b(b'a', digest_size=8).hexdigest())",
        "ba7816bf8f01cfea 9dd4e461 da39a3ee 40f89e395b66422f\n",
    ),
    case(
        "hmac_secrets",
        "import hmac, hashlib, secrets\nprint(hmac.new(b'k', b'm', hashlib.sha256).hexdigest()[:12], len(secrets.token_hex(8)))",
        "b60090e30522 16\n",
    ),
    case(
        "base64_binascii",
        "import base64, binascii\nprint(base64.b64encode(b'hi!'), base64.urlsafe_b64decode('aGk_'), binascii.crc32(b'abc'))",
        "b'aGkh' b'hi?' 891568578\n",
    ),
    case(
        "zlib_gzip",
        "import zlib, gzip\nd = b'x' * 1000\nprint(len(zlib.compress(d)) < 50, zlib.decompress(zlib.compress(d)) == d, gzip.decompress(gzip.compress(b'g')))",
        "True True b'g'\n",
    ),
    case(
        "zlib_module_spec",
        "import importlib.util\nprint(importlib.util.find_spec('zlib') is not None)",
        "True\n",
    ),
    case(
        "struct",
        "import struct\nprint(struct.pack('<IhB', 1, -2, 255), struct.unpack('>H', b'\\x01\\x02'), struct.calcsize('<qd'))",
        "b'\\x01\\x00\\x00\\x00\\xfe\\xff\\xff' (258,) 16\n",
    ),
    case(
        "textwrap_string",
        "import textwrap, string\nprint(textwrap.wrap('aaa bbb ccc', 7), string.Template('$x!').substitute(x='hi'), string.ascii_lowercase[:3])",
        "['aaa bbb', 'ccc'] hi! abc\n",
    ),
    case(
        "difflib",
        "import difflib\nprint(list(difflib.unified_diff(['a\\n', 'b\\n'], ['a\\n', 'c\\n'], lineterm='', n=0))[2:])",
        "['@@ -2 +2 @@', '-b\\n', '+c\\n']\n",
    ),
    case(
        "unicodedata",
        "import unicodedata\nprint(unicodedata.name('é'), unicodedata.normalize('NFD', 'é') == 'e\\u0301', unicodedata.category('A'))",
        "LATIN SMALL LETTER E WITH ACUTE True Lu\n",
    ),
    case(
        "urllib_parse",
        "from urllib.parse import urlparse, urlencode, quote, parse_qs\nu = urlparse('https://h.io:8/p?q=1#f')\nprint(u.hostname, u.port, urlencode({'a': 'b c'}), quote('/a b'), parse_qs('x=1&x=2'))",
        "h.io 8 a=b+c /a%20b {'x': ['1', '2']}\n",
    ),
    case(
        "html_xml",
        "import html\nimport xml.etree.ElementTree as ET\nr = ET.fromstring('<r><i n=\"1\">a</i></r>')\nprint(html.escape('<&>'), r.find('i').get('n'), r.find('i').text)",
        "&lt;&amp;&gt; 1 a\n",
    ),
    case(
        "shlex",
        "import shlex\nprint(shlex.split('a \"b c\" d\\\\ e'), shlex.quote(\"it's\"))",
        "['a', 'b c', 'd e'] 'it'\"'\"'s'\n",
    ),
    case(
        "argparse",
        "import argparse\np = argparse.ArgumentParser(prog='t')\np.add_argument('--n', type=int, default=1)\np.add_argument('files', nargs='*')\nprint(p.parse_args(['--n', '3', 'a', 'b']))",
        "Namespace(n=3, files=['a', 'b'])\n",
    ),
    case(
        "logging",
        "import logging, sys\nlogging.basicConfig(stream=sys.stdout, format='%(levelname)s:%(message)s', level=logging.INFO)\nlogging.getLogger('x').info('hi')\nlogging.debug('hidden')",
        "INFO:hi\n",
    ),
    case(
        "pprint",
        "import pprint\npprint.pprint({'b': [1] * 3, 'a': 'x'}, width=20)",
        "{'a': 'x',\n 'b': [1, 1, 1]}\n",
    ),
    case(
        "heapq_bisect",
        "import heapq, bisect\nh = [5, 1, 3]; heapq.heapify(h)\nprint(heapq.nsmallest(2, h), bisect.bisect([1, 3, 5], 4))",
        "[1, 3] 2\n",
    ),
    case(
        "copy_weakref",
        "import copy, weakref\nclass O: pass\na = [[1]]; b = copy.deepcopy(a); b[0].append(2)\no = O(); r = weakref.ref(o)\nprint(a, r() is o)",
        "[[1]] True\n",
    ),
    case(
        "io_streams",
        "import io\ns = io.StringIO(); print('x', file=s, end='')\nb = io.BytesIO(b'ab'); b.seek(1)\nprint(s.getvalue(), b.read())",
        "x b'b'\n",
    ),
    case(
        "typing_protocols",
        "from typing import Protocol, TypedDict, NamedTuple\nclass T(TypedDict):\n    a: int\nclass N(NamedTuple):\n    x: int = 1\nprint(T(a=1), N())",
        "{'a': 1} N(x=1)\n",
    ),
    case(
        "abc",
        "import abc\nclass A(abc.ABC):\n    @abc.abstractmethod\n    def f(self): ...\ntry:\n    A()\nexcept TypeError:\n    print('abstract')",
        "abstract\n",
    ),
    case(
        "operator_keyword",
        "import operator, keyword\nprint(operator.itemgetter(1)('ab'), keyword.iskeyword('match'), keyword.issoftkeyword('match'))",
        "b False True\n",
    ),
    case(
        "calendar",
        "import calendar\nprint(calendar.isleap(2024), tuple(map(int, calendar.monthrange(2023, 2))))",
        "True (2, 28)\n",
    ),
    case(
        "uuid",
        "import uuid\nu = uuid.uuid4()\nprint(u.version, len(str(u)), uuid.UUID('12345678123456781234567812345678'))",
        "4 36 12345678-1234-5678-1234-567812345678\n",
    ),
    case(
        "random_seeded",
        "import random\nr = random.Random(42)\nprint(r.randint(1, 100), r.choice('abc'))",
        "82 a\n",
    ),
    case(
        "tokenize_ast",
        "import ast\nt = ast.parse('x = 1 + 2')\nprint(type(t.body[0]).__name__, ast.literal_eval('[1, (2, 3)]'))",
        "Assign [1, (2, 3)]\n",
    ),
    case(
        "string_io_csv_sniffer",
        "import csv\nprint(csv.Sniffer().sniff('a;b\\n1;2\\n').delimiter)",
        ";\n",
    ),
    case(
        "sqlite_memory",
        "import sqlite3\nc = sqlite3.connect(':memory:')\nc.execute('create table t(a, b)')\nc.executemany('insert into t values (?, ?)', [(1, 'x'), (2, 'y')])\nprint(c.execute('select sum(a), group_concat(b) from t').fetchone())",
        "(3, 'x,y')\n",
    ),
    case(
        "sqlite_json1",
        "import sqlite3\nprint(sqlite3.connect(':memory:').execute(\"select json_extract('{\\\"a\\\":[1,2]}', '$.a[1]')\").fetchone()[0])",
        "2\n",
    ),
    case(
        "zipfile_in_memory",
        "import zipfile, io\nb = io.BytesIO()\nwith zipfile.ZipFile(b, 'w', zipfile.ZIP_DEFLATED) as z: z.writestr('a.txt', 'hello')\nprint(zipfile.ZipFile(b).read('a.txt'))",
        "b'hello'\n",
    ),
    case(
        "tarfile_in_memory",
        "import tarfile, io\nb = io.BytesIO()\nwith tarfile.open(fileobj=b, mode='w:gz') as t:\n    d = b'data'; i = tarfile.TarInfo('f'); i.size = len(d); t.addfile(i, io.BytesIO(d))\nb.seek(0)\nprint(tarfile.open(fileobj=b).extractfile('f').read())",
        "b'data'\n",
    ),
    case(
        "time_basics",
        "import time\nt0 = time.monotonic(); time.sleep(0.01)\nprint(time.monotonic() - t0 >= 0.01, time.time() > 1.7e9, time.strftime('%Y', time.gmtime(0)))",
        "True True 1970\n",
    ),
    case(
        "warnings_capture",
        "import warnings\nwith warnings.catch_warnings(record=True) as w:\n    warnings.simplefilter('always'); warnings.warn('x', DeprecationWarning)\nprint(len(w), w[0].category.__name__)",
        "1 DeprecationWarning\n",
    ),
    case(
        "traceback_format",
        "import traceback\ntry:\n    1 / 0\nexcept ZeroDivisionError:\n    print(traceback.format_exc().splitlines()[-1])",
        "ZeroDivisionError: division by zero\n",
    ),
    case(
        "inspect",
        "import inspect\ndef f(a, b=2, *c, d): pass\nprint(inspect.signature(f))",
        "(a, b=2, *c, d)\n",
    ),
    case(
        "fnmatch_glob_patterns",
        "import fnmatch\nprint(fnmatch.filter(['a.py', 'b.txt', 'c.py'], '*.py'))",
        "['a.py', 'c.py']\n",
    ),
    case(
        "os_path",
        "import os.path as p\nprint(p.join('a', 'b'), p.splitext('x.tar.gz'), p.normpath('/a/./b/../c'), p.basename('/q/w.e'))",
        "a/b ('x.tar', '.gz') /a/c w.e\n",
    ),
    case(
        "email_message",
        "from email.message import EmailMessage\nm = EmailMessage(); m['Subject'] = 'hi'; m.set_content('body')\nprint(m['Subject'], m.get_content().strip())",
        "hi body\n",
    ),
    case(
        "graphlib_contextvars",
        "import graphlib, contextvars\nprint(list(graphlib.TopologicalSorter({'b': {'a'}}).static_order()), contextvars.ContextVar('v', default=3).get())",
        "['a', 'b'] 3\n",
    ),
    case(
        "array_memoryview",
        "import array\na = array.array('i', [1, 2, 3])\nprint(a.tolist(), bytes(memoryview(b'abc')[1:]))",
        "[1, 2, 3] b'bc'\n",
    ),
    case(
        "locale_codecs",
        "import codecs\nprint(codecs.encode('abc', 'rot13'), 'é'.encode('latin-1'), b'\\xe9'.decode('cp1252'))",
        "nop b'\\xe9' é\n",
    ),
    // --- sandbox-specific behavior ------------------------------------------------
    sandbox_only(
        "version_info",
        "import sys\nprint(sys.version_info[:2], sys.platform, sys.implementation.name)",
        "(3, 14) wasi cpython\n",
    ),
    sandbox_only(
        "sqlite_version_bundled",
        "import sqlite3\nprint(sqlite3.sqlite_version)",
        "3.50.4\n",
    ),
    sandbox_only(
        "subprocess_unavailable",
        "import subprocess\ntry:\n    subprocess.run(['ls'])\nexcept OSError as e:\n    print('OSError')",
        "OSError\n",
    ),
    sandbox_only(
        "threads_unavailable",
        "import threading\ntry:\n    threading.Thread(target=print).start()\nexcept RuntimeError:\n    print('no threads')",
        "no threads\n",
    ),
    sandbox_only(
        "ctypes_unavailable",
        "try:\n    import ctypes\nexcept ImportError:\n    print('no ctypes')",
        "no ctypes\n",
    ),
    sandbox_only(
        "third_party_unavailable",
        "try:\n    import requests\nexcept ImportError:\n    print('no requests')",
        "no requests\n",
    ),
];

async fn run_case(case: &Case) -> (String, String, i32) {
    let mut bash = Bash::builder().cpython().build();
    bash.fs()
        .write_file(std::path::Path::new("/case.py"), case.code.as_bytes())
        .await
        .unwrap();
    let r = bash.exec("python3 /case.py").await.unwrap();
    (r.stdout.to_string(), r.stderr.to_string(), r.exit_code)
}

#[tokio::test]
async fn capability_cases() {
    let mut failures = Vec::new();
    for case in CASES {
        let (stdout, stderr, code) = run_case(case).await;
        if stdout != case.expected || code != 0 {
            failures.push(format!(
                "{}: exit {code}\n  expected: {:?}\n  actual:   {:?}\n  stderr: {}",
                case.name, case.expected, stdout, stderr
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Keep the table honest: the same programs must print the same output under
/// the host's real CPython. Skipped when no `python3` >= 3.11 is installed.
#[test]
fn cases_match_host_python() {
    let Ok(probe) = std::process::Command::new("python3")
        .args(["-c", "import sys; print(sys.version_info >= (3, 11))"])
        .output()
    else {
        eprintln!("skip: no host python3");
        return;
    };
    if String::from_utf8_lossy(&probe.stdout).trim() != "True" {
        eprintln!("skip: host python3 older than 3.11");
        return;
    }
    let mut failures = Vec::new();
    for case in CASES.iter().filter(|c| c.host) {
        let out = std::process::Command::new("python3")
            .args(["-I", "-c", case.code])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        if stdout != case.expected {
            failures.push(format!(
                "{}: host printed {:?}, table expects {:?}\n  stderr: {}",
                case.name,
                stdout,
                case.expected,
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
