# Controlled temporal input abbreviation foundation

This is a design and test-only prerequisite for #815; it does not change SQL
input behavior. Existing timestamp input checks a session-zone abbreviation
before each date keyword, then checks the active global abbreviation set and
finally the keyword. An unbounded history scan on every keyword would impose a
large uninterruptible per-row regression. Changing that order would change the
meaning of a session zone whose abbreviation is also a keyword.

Jev selected a private fallible resolver callback and finite, exact-spelling
keyword negative-cache design for investigation only
(`target/assembly-review/temporal-input-controlled-contract-*`). The public
`DecodeError` API remains unchanged. Any future SQL route must propagate the
originating cancellation and deadline as their original error, hold the exact
session owner through the scan, publish only complete misses after checking
session identity under the slot lock, and preserve current error and token
ordering. Cache entries may represent only exact catalog spellings, not the
keyword parser's truncated-name equivalence: arbitrary names remain supported.
No production input route is enabled by this contract.

PostgreSQL 18.6 rejects `MSK 2012-01-01 00:00:00` with 22007 but accepts
`2012-01-01 MSK 00:00:00` and the suffix form. At Moscow's 2014 fold and 2011
gap, both accepted orders resolve to 22:30 UTC on the previous day. An invalid
date followed by MSK returns 22008; an invalid token returns 22007. PostgreSQL's
`DecodeDateTime` checks timezone abbreviations before built-in keywords and its
DYNTZ handling retains the abbreviation for later resolution. The oracle was
stopped after these probes. Tests in this branch are authored and unexecuted.

Open prerequisites include dynamic abbreviation classification and final-date
resolution, token and diagnostic precedence, keyword/abbreviation cache
capacity and lifetime, actual SQL-cast controls routing, preexisting tokenizer
allocation and interruption behavior, other temporal inputs, public callers,
and the first/full-history scan. An immutable source checkpoint or a Jev
selection is not executable qualification or #815 completion.
