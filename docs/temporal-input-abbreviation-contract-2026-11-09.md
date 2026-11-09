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


A separate raw-TZif parity boundary applies to both the old bounded lookup and
the integrated checked lookup. PostgreSQL's `pg_timezone_abbrev_is_known`
scans every raw local-time type, including an unreferenced nonzero type; Jiff's
public preceding/following transition API exposes transition-reachable types.
The pinned jiff-core 0.1.2 parser explicitly accepts unreferenced types. A
synthetic, unexecuted witness compares the old fallback and new cached fallback
for such a custom zone; PostgreSQL source would recognize the orphan CST type.
Cassie's statement settings resolve names through Jiff's configured database or
POSIX parsing; a direct custom `Zone::Rules` is public but is not itself proof
that SQL settings can install the synthetic instance. Jiff features can unify in
downstream consumers, so bundled-zone provenance must not be assumed. Proving
real setting reachability and a public way to inspect the actual zone's complete
raw type table remain open. No new classification is promoted on this basis.

Jev selected an isolated test-only classifier as the next step
(`target/assembly-review/temporal-dynamic-classification-contract-*`). A
reachable-interval scan compares both offset and daylight-saving flag, checks
interruption during traversal, and classifies a known spelling as fixed or
varying. The PostgreSQL raw-type distinction, final-date application, token
masks, cache, SQL-cast wiring, owner and resource bounds remain unproved. The
bundled data inspection cannot discriminate all actual Jiff zone databases; no
production path is selected or enabled by this prototype.

PostgreSQL 18.6 also treats `AAA0AAA-1,M3.2.0,M11.1.0` as fixed UTC for
`AAA`: both January and July inputs and typed `timezone` overloads resolve
12:00 to 12:00 UTC. A reachable-transition classifier instead sees the same
spelling at offsets 0 and +01 and identifies a dynamic abbreviation. The
operand-time route was withdrawn in a separate corrective checkpoint.

The paired unexecuted TZif witness varies only whether two local-time types
reference a shared `AAA` designation or two distinct `AAA` designation indices.
PostgreSQL's raw index scan selects the first matching index: the first file is
dynamic and the second is fixed. The pinned Jiff parser interns equal strings,
so the public offset/transition view loses this distinction. Jiff equality also
uses a raw-data CRC32, so this is not a claim that every public operation sees
the same object; a checksum is not collision-free access to designation indices.
A correct promotion must preserve or obtain the actual source designation
identity; no such Jiff API or safe provenance discriminator has yet been shown.
