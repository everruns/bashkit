"""requests.utils (bashkit): the helpers scripts reach for."""

from urllib.parse import quote, unquote, urlparse  # noqa: F401

from .sessions import default_headers, default_user_agent  # noqa: F401


def dict_from_cookiejar(cj):
    return dict(cj)


def add_dict_to_cookiejar(cj, cookie_dict):
    for k, v in cookie_dict.items():
        cj[k] = v
    return cj


def requote_uri(uri):
    return quote(unquote(uri), safe="!#$%&'()*+,/:;=?@[]~")
