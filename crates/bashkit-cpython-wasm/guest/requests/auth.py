"""requests.auth (bashkit): basic auth and the AuthBase hook."""

from _bashkit_webcore import basic_auth


class AuthBase:
    def __call__(self, r):
        raise NotImplementedError("Auth hooks must be callable.")


class HTTPBasicAuth(AuthBase):
    def __init__(self, username, password):
        self.username = username
        self.password = password

    def __eq__(self, other):
        return (self.username, self.password) == (
            getattr(other, "username", None),
            getattr(other, "password", None),
        )

    def __call__(self, r):
        r.headers["Authorization"] = basic_auth(self.username, self.password)
        return r


class HTTPProxyAuth(HTTPBasicAuth):
    def __call__(self, r):
        return r  # proxies are not used in bashkit


def _basic_auth_str(username, password):
    return basic_auth(username, password)
