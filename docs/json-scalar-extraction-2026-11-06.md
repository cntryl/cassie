# JSON scalar extraction identity

Issue #809 covers `->` and `->>` identity and NULL semantics. PostgreSQL 18.6
returns SQL NULL for integer indexing of a scalar `json` value, including
indexes 0 and -1. Its `jsonb` carrier instead treats a scalar as a one-element
array for those two indexes; JSON document null remains a JSONB value with `->`
and becomes SQL NULL with `->>`. Actual arrays retain existing behavior.

The prior evaluator applied the JSONB scalar rule to both carriers even though
the binder preserves their identities. The focused pgwire regression covers
values and result OIDs and passed in the 2026-11-10 complete locked test suite.
No observed pre-fix RED is claimed. Native oracle request/results are retained
under ignored `target/v020-json/`. No persistent format, Midge API or
PostgreSQL-wide support changes.

Focused Jev review assigned 0.70 to chained carrier identity, 0.65 to typed
parameter identity, and 0.34 to document-null handling. PostgreSQL 18.6
confirmed JSON chains/typed parameters return SQL NULL while JSONB preserves
the scalar or document null. Additional chain and explicitly typed OID 114/3802
parameter regressions were authored after the initial implementation in response
to those leads; they passed in the 2026-11-10 complete locked test suite. Their
oracle output is retained in ignored
`target/v020-json/oracle-scalar-params.json`. Existing tests cover JSONB chain
semantics. Broader JSON support remains Experimental as recorded in Feature
Support.
