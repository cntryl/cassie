# JSON scalar extraction identity

Issue #809 covers `->` and `->>` identity and NULL semantics. PostgreSQL 18.6
returns SQL NULL for integer indexing of a scalar `json` value, including
indexes 0 and -1. Its `jsonb` carrier instead treats a scalar as a one-element
array for those two indexes; JSON document null remains a JSONB value with `->`
and becomes SQL NULL with `->>`. Actual arrays retain existing behavior.

The prior evaluator applied the JSONB scalar rule to both carriers even though
the binder preserves their identities. The focused pgwire regression was authored
before the evaluator correction and covers values and result OIDs. It is not yet
executed; no observed pre-fix RED or final qualification is claimed. Native
oracle request/results are retained under ignored `target/v020-json/`. Runtime
builds, tests and final review remain deferred until the milestone bundle is
assembled. No persistent format, Midge API or PostgreSQL-wide support changes.
