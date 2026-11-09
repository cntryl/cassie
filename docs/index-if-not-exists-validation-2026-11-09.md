# Duplicate index column validation

PostgreSQL 18.6 checks a missing predicate column before a missing key column,
and a missing key column before a missing INCLUDE column, even when
`CREATE INDEX IF NOT EXISTS` names an existing index. It skips a duplicate
statement with a key also repeated in INCLUDE. Unknown storage parameters can
outrank a missing key, and invalid functions or access methods have distinct
diagnostics. These observations came from a separate PostgreSQL oracle; the
oracle was stopped afterward.

Jev selected a bounded duplicate-only first increment
(`target/assembly-review/index-duplicate-validation-subset-*`). Its scope is
an existing index on the same table, scalar btree syntax, no options
or key expressions, and an absent predicate or a single column/literal binary
predicate (or a single column), with only unqualified referenced columns. A recognized predicate's missing columns are
checked first. If its columns exist, the prior skip remains because predicate
typing can itself fail before key validation. With no predicate, check key
columns then INCLUDE columns, returning `42703` for the first missing column.
Valid duplicates continue through the prior no-op. Other AST forms keep
their existing behavior and remain pending; #842 is not complete. The
nonduplicate diagnostic path remains unchanged.

Additional PostgreSQL probes confirmed missing unqualified, qualified and
quoted predicate references return `42703`; a wrong qualifier can instead
produce a distinct relation error. Qualified references are therefore left to
the existing skip pending the full binder. An ill-typed `1 + true` predicate
returns `42883` before a missing key; key checks after a present predicate are
also deferred. The oracle was stopped after these probes.

This new duplicate traversal must preserve query cancellation and account for
its schema/name/stack owners; these are source obligations, not guarantees
established by the PostgreSQL oracle or the unexecuted Cassie tests.
The draft carries originating binder controls and uses no traversal stack.
The existing `clone_schema_with_controls` still has an unchecked fallback name
scan and plain schema clone after estimated admission. Column-key construction
and `UndefinedColumn`'s returned diagnostic String also lack owner-coupled
admission; cancellation during those operations and those exact allocations
remain pending. Public standalone binder calls have no supplied query controls.
