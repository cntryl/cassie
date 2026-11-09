# Timestamp binary Bind extra-data diagnostic

PostgreSQL 18.6 reports valid TIMESTAMP/TIMESTAMPTZ binary prefixes followed
by unread data as 22P03 `incorrect binary data format in bind parameter N`,
where N is the one-based parameter index. An out-of-range prefix still reports
22008 before extra-data handling. Array element errors retain their own element
ordinal; NULL parameters have no binary body to decode.

The Bind handler already enumerates parameters. Pass that index through its
private scalar decoding functions and rewrite only a structured scalar
TIMESTAMP/TIMESTAMPTZ `ExcessBinaryData` error. Leave typed range, short-frame,
array element and other codec errors on their existing paths; the internal
benchmark helper retains its public signature and supplies index zero. This
corrects the explicitly recorded gap in
`temporal-binary-receive-bounds-2026-11-09.md`. Authored wire probes covering
first/second parameters, NULL, error priority and implicit/explicit recovery
are unexecuted pending consolidated validation.
