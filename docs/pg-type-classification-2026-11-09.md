# Built-in `pg_type` classification

PostgreSQL 18.6 returns `typtype` and `typcategory` OID 18 for direct
`pg_type` fields. The observed (kind, category) pairs include bool `(b,B)`,
int4 `(b,N)`, text `(b,S)`, unknown `(p,X)`, _int4 `(b,A)`, regclass
`(b,N)` and jsonb `(b,U)`. The existing PostgreSQL 18.6 builtin identity
registry contains these values; the catalog previously synthesized `a` for
array kind and `b/Z` for unknown.

For existing registered builtin identities, return their pinned kind and
category. Preserve the previous fallback for private/vector identities and do
not synthesize user-defined types. Direct, aliased and wildcard-only projections
from `pg_catalog.pg_type` report category as internal `char` OID 18, matching
the existing `typtype` treatment. This narrow category descriptor change does
not infer ownership through joins, CTEs, subqueries, or mixed wildcard and
expression projections. A joined unrelated TEXT column and a computed
duplicate `typcategory` alias retain their original descriptors.
The prior `typtype` path and other catalog descriptor limitations remain open.

Focused catalog and binary wire probes are authored but unexecuted. This does
not complete #817, change persistent data or introduce catalog rows. The full
consolidated validation gate remains deferred.
