# Transactional scalar index replacement

This is a source and oracle investigation for #833 and #842, not a completed
support claim. PostgreSQL 18.6 in an isolated temporary-table session accepts
`DROP INDEX` followed by a same-name unique index on another field in one
transaction. The replacement is visible to that transaction. Rolling back to a
savepoint restores the old definition; repeating the replacement and committing
enforces the new unique field and permits values unique only under the old one.
The oracle process was stopped after the probe. The new Cassie tests are authored
but have not been run.

Current Cassie defers index cleanup using a durable table/name record. Its
cleanup resolves the then-current index by that name, deletes its reservation
prefix, and deletes its physical scalar-index data and metadata. A replacement
prepared while the old metadata remains also reuses that old physical storage ID,
and unique reservations default to the same index name. A name-only identity
check on cleanup is therefore insufficient to protect old readers or a new
same-name index. This is a source-backed hazard, not an observed Cassie result.

One candidate gives the replacement a fresh physical index identity and unique
reservation namespace, stages removal of the old name mapping, and persists an
old-identity cleanup record that can retire only the old physical prefixes after
the epoch guard drains. It needs an explicit backward-compatible cleanup-record
and restart contract, atomic old/new catalog/storage publication, savepoint and
failed-statement behavior, concurrent creation/drop fences, index catalog OID
semantics and cancellation-safe cleanup. Existing name-only records and absent or
legacy identities need a safe rule. Published Midge 0.5.2 remains the sole storage
engine; no upstream change, second layer or unproved atomic capability is assumed.
Do not implement a durable identity change until the contract and its concrete
preconditions have been selected and recorded.
