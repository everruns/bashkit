"""requests.adapters (bashkit).

Every request goes through the bashkit host, so adapters only exist for
compatibility: `Session.mount()` accepts them and they change nothing.
Retries (`max_retries`) are not performed.
"""


class BaseAdapter:
    def close(self):
        pass


class HTTPAdapter(BaseAdapter):
    def __init__(self, pool_connections=10, pool_maxsize=10, max_retries=0, pool_block=False):
        self.max_retries = max_retries
        self.config = {}

    def init_poolmanager(self, *args, **kwargs):
        pass


DEFAULT_POOLSIZE = 10
DEFAULT_RETRIES = 0
