"""Small libpq binding preserving descriptors and raw text/binary bytes.

Uses asynchronous sends and bounded waits. This is test infrastructure; Cassie's
Rust parser/executor and direct Midge persistence are unchanged.
"""

import ctypes as c
import select
import time


class Libpq:
    def __init__(self, path):
        self.api = c.CDLL(str(path))
        p, i, s, u = c.c_void_p, c.c_int, c.c_char_p, c.c_uint
        signatures = {
            "PQconnectdb": (p, [s]), "PQstatus": (i, [p]),
            "PQfinish": (None, [p]), "PQsocket": (i, [p]),
            "PQsetnonblocking": (i, [p, i]), "PQflush": (i, [p]),
            "PQconsumeInput": (i, [p]), "PQisBusy": (i, [p]),
            "PQgetResult": (p, [p]), "PQclear": (None, [p]),
            "PQsendQuery": (i, [p, s]),
            "PQsendPrepare": (i, [p, s, s, i, c.POINTER(u)]),
            "PQsendDescribePrepared": (i, [p, s]),
            "PQsendQueryPrepared": (i, [p, s, i, c.POINTER(s), c.POINTER(i), c.POINTER(i), i]),
            "PQresultStatus": (i, [p]), "PQresultErrorField": (s, [p, i]),
            "PQcmdStatus": (s, [p]), "PQnfields": (i, [p]),
            "PQntuples": (i, [p]), "PQfname": (s, [p, i]),
            "PQftype": (u, [p, i]), "PQfmod": (i, [p, i]),
            "PQfsize": (i, [p, i]), "PQfformat": (i, [p, i]),
            "PQgetisnull": (i, [p, i, i]), "PQgetlength": (i, [p, i, i]),
            "PQgetvalue": (p, [p, i, i]), "PQnparams": (i, [p]),
            "PQparamtype": (u, [p, i]), "PQserverVersion": (i, [p]),
        }
        for name, (result, args) in signatures.items():
            function = getattr(self.api, name)
            function.restype, function.argtypes = result, args

    def connect(self, dsn):
        return Connection(self.api, dsn)


class Connection:
    def __init__(self, api, dsn):
        self.api = api
        self.handle = api.PQconnectdb((dsn + " connect_timeout=5").encode())
        if not self.handle or api.PQstatus(self.handle) != 0:
            self.close()
            # Do not include credentials or connection strings in evidence.
            raise ConnectionError("libpq connection unavailable")
        if api.PQsetnonblocking(self.handle, 1) != 0:
            self.close()
            raise ConnectionError("libpq nonblocking mode unavailable")

    def close(self):
        if self.handle:
            self.api.PQfinish(self.handle)
            self.handle = None

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()

    def _column(self, result, index):
        api = self.api
        raw_name = api.PQfname(result, index)
        try:
            display_name = raw_name.decode("utf-8")
        except UnicodeDecodeError:
            display_name = None
        return {"name": display_name, "name_hex": raw_name.hex(),
                "oid": api.PQftype(result, index), "typmod": api.PQfmod(result, index),
                "size": api.PQfsize(result, index), "format": api.PQfformat(result, index)}

    def _snapshot(self, result):
        api = self.api
        count = api.PQnfields(result)
        state = api.PQresultErrorField(result, ord("C"))
        return {
            "status": api.PQresultStatus(result),
            "sqlstate": state.decode() if state else None,
            "command_tag": (api.PQcmdStatus(result) or b"").decode(),
            "columns": [self._column(result, col) for col in range(count)],
            "rows": [
                [None if api.PQgetisnull(result, row, col) else
                 c.string_at(api.PQgetvalue(result, row, col), api.PQgetlength(result, row, col)).hex()
                 for col in range(count)]
                for row in range(api.PQntuples(result))
            ],
            "parameter_oids": [api.PQparamtype(result, index) for index in range(api.PQnparams(result))],
        }

    def _receive(self, sent):
        if sent != 1:
            raise ConnectionError("libpq send failed")
        deadline = time.monotonic() + 10
        results = []
        while True:
            pending = self.api.PQflush(self.handle)
            if pending < 0:
                raise ConnectionError("libpq flush failed")
            if not pending and not self.api.PQisBusy(self.handle):
                result = self.api.PQgetResult(self.handle)
                if not result:
                    return results
                try:
                    status = self.api.PQresultStatus(result)
                    # COPY needs a dedicated streaming adapter; never hang or count it as passed.
                    if status in (3, 4, 8):
                        raise NotImplementedError("COPY streaming adapter is pending")
                    results.append(self._snapshot(result))
                finally:
                    self.api.PQclear(result)
                continue
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("query exceeded the harness deadline")
            fd = self.api.PQsocket(self.handle)
            ready, _, _ = select.select([fd], [fd] if pending else [], [], remaining)
            if ready and self.api.PQconsumeInput(self.handle) != 1:
                raise ConnectionError("libpq receive failed")

    def query(self, sql, mode="simple_text", parameters=()):
        if mode == "simple_text":
            if parameters:
                raise ValueError("simple protocol cannot bind parameters")
            return self._receive(self.api.PQsendQuery(self.handle, sql.encode()))
        if mode not in ("extended_text", "extended_binary"):
            raise ValueError("unknown protocol mode")
        types = (c.c_uint * len(parameters))(*(item["oid"] for item in parameters))
        prepared = self._receive(self.api.PQsendPrepare(self.handle, b"", sql.encode(), len(parameters), types))
        if len(prepared) != 1 or prepared[0]["status"] != 1:
            return prepared
        described = self._receive(self.api.PQsendDescribePrepared(self.handle, b""))
        if len(described) != 1 or described[0]["status"] != 1:
            return described
        values = [None if item["value"] is None else bytes.fromhex(item["value"]) for item in parameters]
        # Keep buffers alive through receipt; preserve embedded NUL in binary parameters.
        buffers = [None if value is None else c.create_string_buffer(value) for value in values]
        pointers = (c.c_char_p * len(parameters))(*(None if buf is None else c.cast(buf, c.c_char_p) for buf in buffers))
        lengths = (c.c_int * len(parameters))(*(0 if value is None else len(value) for value in values))
        formats = (c.c_int * len(parameters))(*(item["format"] for item in parameters))
        result = self._receive(self.api.PQsendQueryPrepared(
            self.handle, b"", len(parameters), pointers, lengths, formats, int(mode == "extended_binary")))
        for item in result:
            item["parameter_oids"] = described[0]["parameter_oids"]
            item["prepared_columns"] = described[0]["columns"]
        return result
