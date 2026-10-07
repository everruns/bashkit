"""requests.structures (bashkit)."""

from _bashkit_webcore import CaseInsensitiveDict


class LookupDict(dict):
    def __init__(self, name=None):
        self.name = name
        super().__init__()

    def __getitem__(self, key):
        return self.__dict__.get(key, None)

    def get(self, key, default=None):
        return self.__dict__.get(key, default)


__all__ = ["CaseInsensitiveDict", "LookupDict"]
