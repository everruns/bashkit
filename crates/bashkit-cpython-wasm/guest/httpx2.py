"""httpx2 (pydantic/httpx2) for bashkit's CPython: the same module as
`httpx` (see httpx.py), registered under both names."""

import sys

import httpx

sys.modules[__name__] = httpx
