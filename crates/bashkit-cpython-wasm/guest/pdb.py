"""pdb stand-in for bashkit's CPython guest.

There is no interactive terminal in the sandbox, so the real debugger is
not shipped. This stub keeps scripts that call it running: `breakpoint()`
and `pdb.set_trace()` print a notice and continue; `pdb.run*` raise
RuntimeError.
"""

import sys

_MSG = "pdb: debugger not available in the bashkit sandbox; continuing"


class Pdb:
    """Constructible; debugging is a no-op."""

    def __init__(self, *args, **kwargs):
        pass

    def set_trace(self, *args, **kwargs):
        print(_MSG, file=sys.stderr)

    def reset(self):
        pass

    def set_continue(self):
        pass


def set_trace(*args, header=None, **kwargs):
    print(_MSG, file=sys.stderr)


def post_mortem(*args, **kwargs):
    print(_MSG, file=sys.stderr)


def pm():
    post_mortem()


def run(*args, **kwargs):
    raise RuntimeError("pdb is not available in the bashkit sandbox")


runeval = runcall = runctx = run
