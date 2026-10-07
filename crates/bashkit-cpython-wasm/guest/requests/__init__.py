"""requests for bashkit's CPython: the common `requests` API over the host's
HTTP egress pipeline (see `_bashkit_webcore`).

Supported: the verb helpers, `Session` (headers, params, auth, cookies,
hooks, mount), params/data/json/files/headers/cookies/auth/timeout/
allow_redirects, `Response` (status_code, ok, reason, headers, content,
text, json(), encoding, url, history, links, iter_content/iter_lines,
raise_for_status), and the upstream exception hierarchy.
Accepted and ignored: verify, cert, proxies, stream (bodies are buffered),
adapters and their retries.
"""

__title__ = "requests"
__version__ = "2.32.0+bashkit"

from . import exceptions, status_codes, structures  # noqa: E402
from .api import delete, get, head, options, patch, post, put, request  # noqa: E402
from .exceptions import (  # noqa: E402
    ConnectionError,
    ConnectTimeout,
    FileModeWarning,
    HTTPError,
    JSONDecodeError,
    ReadTimeout,
    RequestException,
    Timeout,
    TooManyRedirects,
    URLRequired,
)
from .models import PreparedRequest, Request, Response  # noqa: E402
from .sessions import Session, session  # noqa: E402
from .status_codes import codes  # noqa: E402


def __getattr__(name):
    if name == "utils":
        from . import utils as mod

        return mod
    raise AttributeError(name)
