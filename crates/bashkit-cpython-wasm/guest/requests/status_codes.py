"""requests.status_codes (bashkit): `codes.ok`, `codes.not_found`, ..."""

from http import HTTPStatus

from .structures import LookupDict

codes = LookupDict(name="status_codes")
for _s in HTTPStatus:
    _name = _s.name.lower()
    setattr(codes, _name, _s.value)
    setattr(codes, _s.name, _s.value)
for _alias, _value in {
    "ok": 200, "okay": 200, "all_ok": 200, "all_good": 200, "\\o/": 200,
    "found": 302, "moved": 301, "temporary_redirect": 307, "permanent_redirect": 308,
    "bad": 400, "unauthorized": 401, "forbidden": 403, "not_found": 404,
    "not_allowed": 405, "conflict": 409, "teapot": 418, "too_many_requests": 429,
    "server_error": 500, "internal_server_error": 500, "unavailable": 503,
}.items():
    setattr(codes, _alias, _value)
del _s, _name, _alias, _value
