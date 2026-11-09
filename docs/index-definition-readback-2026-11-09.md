# Index definition guard on metadata readback

The durable expression contract requires rejecting prohibited subqueries in
serialized index expression and predicate ASTs when metadata is read back.
Index publication already calls `definition_guard::index`; `get_index`'s exact
and legacy scan branches and `list_indexes` currently check only the persisted
semantics version and can return an unsafe record.

Apply the same index definition guard at each returned-record boundary, after
the existing explicit version check. Preserve exact-key malformed JSON as the
existing parse error, skip malformed records in scan/list as before, examine
only a matching record on fallback get, and retain unsupported-version error
precedence over a guarded expression. Valid legacy and current indexes retain
their existing format and read/publication behavior. The normal in-memory Midge
fixture is sufficient for these byte/readback boundaries; no Midge engine or
persistent format change is selected.

Test-first cases seed raw legacy bytes through the existing Cassie Midge adapter
and cover expression/predicate subqueries, filtered and unfiltered windows,
exact and fallback gets, list reads, malformed records, valid metadata, future
version precedence and ordinary publication. All are unexecuted pending the
combined build/test gate; this increment does not qualify unrelated carriers.
