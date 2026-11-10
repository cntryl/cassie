# Partial indexes with storage options

Issue #842 includes selected btree options and partial-index maintenance. A
PostgreSQL 18.6 reference accepted `CREATE INDEX ON t (n) WITH (fillfactor=80)
WHERE n > 0`. Cassie's parser previously consumed the entire remainder as an
option list and rejected this combination. The source change bounds the option
list at its matching closing parenthesis and parses the following predicate.
Other consumers of the shared option parser continue to reject unexpected
trailing tokens.

The parser probes and the existing partial-index lifecycle fixture were authored
or amended before the implementation. They have not been executed. A focused
source and Jev review found no confirmed regression; it inspected the shared
parser callers and quoted, nested, and malformed parenthesis boundaries. The
combined local build, locked test suite, strict Clippy, formatting, and test-policy
gates remain pending. This increment does not complete #842.
