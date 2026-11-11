# Controlled temporal input abbreviation foundation

This contract records selected ordinary temporal input increments for #815
and the remaining qualification boundary. Input checks a session-zone
abbreviation before each date keyword, then the selected global abbreviation
set, and finally the keyword. The selected Default MSK route uses bounded
abbreviation lookup; other exact dynamic aliases already present in the
PostgreSQL 18.6 formatting catalog use their configured IANA zone after the
complete input date is known. An unbounded history scan on every keyword would
impose a large uninterruptible per-row regression. Changing that order would
change the meaning of a session zone whose abbreviation is also a keyword.

Jev selected a private fallible resolver callback and finite, exact-spelling
keyword negative-cache design for investigation only
(`target/assembly-review/temporal-input-controlled-contract-*`). The public
`DecodeError` API remains unchanged. A future checked SQL route must propagate
the originating cancellation and deadline as their original error, hold the
exact session owner through any history scan, publish only complete misses
after checking session identity under the slot lock, and preserve current error
and token ordering. Cache entries may represent only exact catalog spellings,
not the keyword parser's truncated-name equivalence: arbitrary names remain
supported. The selected bounded catalog route below does not enable the
general checked-tokenizer or history-scan prototype.

PostgreSQL 18.6 rejects `MSK 2012-01-01 00:00:00` with 22007 but accepts
`2012-01-01 MSK 00:00:00` and the suffix form. At Moscow's 2014 fold and 2011
gap, both accepted orders resolve to 22:30 UTC on the previous day. An invalid
date followed by MSK returns 22008; an invalid token returns 22007. PostgreSQL's
`DecodeDateTime` checks timezone abbreviations before built-in keywords and its
DYNTZ handling retains the abbreviation for later resolution. The oracle was
stopped after these probes. Tests for the older generalized-decoder prototype
remain authored and unexecuted; the separate selected MSK SQL regressions and
their execution evidence are recorded below.

The selected Default MSK and exact catalogued dynamic aliases have bounded
final-date SQL routes. Open prerequisites include raw-TZif abbreviation
classification for arbitrary session-zone sources, token and diagnostic
precedence across remaining input forms, keyword/abbreviation cache capacity
and lifetime, actual SQL-cast controls routing, preexisting tokenizer
allocation and interruption behavior, other temporal inputs, public callers,
and the first/full-history scan. An immutable source checkpoint or a Jev
selection is not executable qualification or #815 completion.


A separate raw-TZif parity boundary applies to both the old bounded lookup and
the earlier, subsequently withdrawn checked lookup. PostgreSQL's `pg_timezone_abbrev_is_known`
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

## Selected Default MSK text-input increment

For the #815 closeout slice, promote only the exact `MSK` entry in PostgreSQL
18.6's `Default` abbreviation set. The pinned primary configuration maps
`MSK` to `Europe/Moscow` (`src/timezone/tznames/Default`, `REL_18_6`). Jev
selected this bounded contract at confidence 1.00; its request and response are
retained in `target/assembly-review/815-selected-mosk-contract-{request,response}.json`.

With session `TimeZone = 'UTC'`, the selected complete-date ordinary
`TIMESTAMPTZ` inputs must resolve the middle and suffix spellings against the
final date/time. The PostgreSQL 18.6 witnesses are:

| Input | Expected UTC instant or result |
| --- | --- |
| `TIMESTAMPTZ '2012-01-01 MSK 00:00:00'` | `2011-12-31 20:00:00+00` |
| `TIMESTAMPTZ '2012-01-01 00:00:00 MSK'` | `2011-12-31 20:00:00+00` |
| `TIMESTAMPTZ '2015-01-01 MSK 12:00:00'` | `2015-01-01 09:00:00+00` |
| `TIMESTAMPTZ '2011-03-27 02:30:00 MSK'` | `2011-03-26 22:30:00+00` |
| `TIMESTAMPTZ '2011-03-27 MSK 02:30:00'` | `2011-03-26 22:30:00+00` |
| `TIMESTAMPTZ '2014-10-26 01:30:00 MSK'` | `2014-10-25 22:30:00+00` |
| `TIMESTAMPTZ '2014-10-26 MSK 01:30:00'` | `2014-10-25 22:30:00+00` |
| `TIMESTAMPTZ 'MSK 2012-01-01 00:00:00'` | reject with `22007` |
| `TIMESTAMPTZ '2012-02-30 MSK 00:00:00'` | reject with `22008` |
| `TIMESTAMPTZ '2012-01-01 NOT_A_ZONE 00:00:00'` | reject with `22007` |

The route must retain session-zone-before-global precedence, reject the
abbreviation before a complete date, and validate the date before final zone
resolution. Cassie may use the named `Europe/Moscow` zone for this configured
global entry; the route does not infer abbreviation identity from arbitrary
session-zone TZif data. It uses the existing bounded abbreviation lookup
(current type plus up to 64 preceding and 64 following transitions), with the
fixed Default offset as fallback; it introduces no full-history scan or
input-sized retained state. This proves only the named 2011/2012/2014 MSK
witnesses above and the 2015 standard-time witness, not the full historical
nearest-or-oldest behavior for every date. Other dynamic entries, non-Default
sets, generic ordinary
`date`/`timestamp`/`timestamptz` abbreviation parity, custom TZif source
identity, and a shared controlled tokenizer/SQL handoff remain pending. This
selected MSK increment does not close #815 or withdraw the punt below.

Four focused SQL regressions in
`tests/sql_queries/types/temporal/text_timezone_abbreviations.rs` cover the
complete-date, gap, fold and diagnostic cases above. They passed in the locked
full test suite, and the touched-test validator accepted the test module.

## Selected Default catalogued dynamic aliases

Ordinary `date`, `timestamp` and `timestamptz` input now recognizes exact
dynamic entries in the existing PostgreSQL 18.6 `Default` formatting catalog,
after checking the session-zone abbreviation. This is a finite name-to-IANA
mapping, not raw-TZif classification: the decoder requires a complete date,
retains the named zone through calendar validation, then uses its local offset
for the final fields. The selected MSK branch above retains its previously
qualified abbreviation-specific behavior. The catalog lookup checks exact
ASCII tokens without creating a token copy or scanning zone history. The
catalog is [`format_zone_catalog.rs`](../src/types/extended/temporal/format_zone_catalog.rs);
the SQL route is in [`decode.rs`](../src/types/extended/temporal/decode.rs).

Jev selected the catalogued dynamic alias route at confidence 1.00 and selected
final local-zone offset resolution for non-MSK entries at confidence 1.00;
receipts are `target/assembly-review/815-dynamic-catalog-abbreviation-route-{request,response}.json`
and `target/assembly-review/815-dynamic-alias-resolution-rule-{request,response}.json`.
The PostgreSQL 18.6 primary oracle ([`datetime.c`](https://github.com/postgres/postgres/blob/REL_18_6/src/backend/utils/adt/datetime.c)
and [`tznames/Default`](https://github.com/postgres/postgres/blob/REL_18_6/src/timezone/tznames/Default))
confirms the resolver computes the named-zone offset after the complete local
date is available and rejects a dynamic abbreviation before a complete date.
With `TimeZone = 'UTC'`, its concrete CLT witnesses are:

| Input | Expected result |
| --- | --- |
| `DATE '2024-01-15 CLT'` | `2024-01-15` |
| `TIMESTAMP '2024-01-15 12:00:00 CLT'` | `2024-01-15 12:00:00` |
| `TIMESTAMPTZ '2024-01-15 12:00:00 CLT'` | `2024-01-15 15:00:00+00` |
| `TIMESTAMPTZ '2024-07-15 12:00:00 CLT'` | `2024-07-15 16:00:00+00` |
| `TIMESTAMPTZ '2024-01-15 12:00:00 cLt'` | `2024-01-15 15:00:00+00` |
| `TimeZone = 'CLT0'`, then `TIMESTAMPTZ '2024-01-15 12:00:00 CLT'` | `2024-01-15 12:00:00+00` |
| `TIMESTAMPTZ 'CLT 2024-01-15 12:00:00'` | reject with `22007` |
| `TIMESTAMPTZ '2024-02-30 CLT 12:00:00'` | reject with `22008` |
| `TIMESTAMPTZ '2024-01-15 12:00:00 CLTx'` | reject with `22007` |

The owning regressions are authored in
[`text_timezone_abbreviations.rs`](../tests/sql_queries/types/temporal/text_timezone_abbreviations.rs); they remain
unexecuted until the frozen ordered bundle gate. The examples prove CLT's
seasonal offset, the selected route across three input types, session-zone
precedence, case-insensitive exact-token matching, complete-date ordering and
invalid-date precedence. They do not qualify every catalog entry. Non-Default abbreviation
sets and complete generic `date`/`timestamp`/`timestamptz` abbreviation parity,
raw-TZif designation identity, checked tokenization, cancellation/ownership
routing and diagnostic-owner handoff remain open under #815. The catalogued
increment does not complete #815 or withdraw the punt below.

Prepared text-parameter regressions are authored in
[`abbreviation_input.rs`](../tests/pgwire_extended/functions/temporal/abbreviation_input.rs).
They check the three temporal output OIDs and resolved values, then verify that
an invalid-date error returns 22008 without a row or command-complete frame
and that the prepared timestamp can run successfully afterward. These wire
probes are also unexecuted until the frozen ordered bundle gate.

An oracle-only sweep compared the other 49 dynamic Default catalog entries to
their mapped IANA zone at local noon on 2024-01-15 and 2024-07-15; PostgreSQL
18.6 returned the same UTC instant for all 98 comparisons. This confirms those
sample points only and is not Cassie execution or per-alias SQL coverage.

## Selected narrow punt and implementation dependency

Full PostgreSQL-compatible session-zone abbreviation handling for ordinary
`date`, `timestamp` and `timestamptz` text input is **punted** pending a
source-coupled raw designation identity and a controlled tokenizer/SQL owner
flow. This is a specific open #815 obligation, not a claim that all timezone
handling is impossible or unsupported. Keep current behavior for unproved
paths; do not replace it with a reachable-transition approximation, a
name/checksum guess, or a restriction that rejects otherwise supported zones.

The consolidated candidate separately contains the narrowly scoped typed
`timezone()` POSIX provenance route described in its
`docs/posix-abbreviation-provenance-2026-11-09.md`.
It covers only its documented typed timestamp/timestamptz operands and verified
rule grammar; it is preserved by this decision. This older prototype branch
contains the subsequently withdrawn broader operand-time route and must not be
cherry-picked wholesale. Neither that withdrawal nor the scoped POSIX route
solves normal temporal text input, arbitrary TZif sources, or #815 as a whole.

PostgreSQL 18.6's
[`localtime.c`](https://github.com/postgres/postgres/blob/REL_18_6/src/timezone/localtime.c)
first finds the matching raw designation index and scans every local-time type,
including unreferenced types. Its
[`datetime.c`](https://github.com/postgres/postgres/blob/REL_18_6/src/backend/utils/adt/datetime.c)
checks abbreviations before ordinary keywords and defers dynamic abbreviation
offset resolution until the final date is known. The branch prototype probes
using partially accumulated date fields or the transaction start and cannot
preserve all of that ordering. The selected MSK operand, gap/fold and invalid
input witnesses have SQL execution coverage as described above; they do not
qualify the generic raw-TZif and controlled-tokenizer paths.

Pinned Jiff 0.2.39 exposes `TimeZone::get`, `TimeZone::tzif`, `iana_name`,
`preceding` and `following`, but not the raw designation table of an existing
zone. Its jiff-core 0.1.2 TZif parser interns equal abbreviation strings; the
public transition view also omits unreferenced local-time types. A zone name or
CRC32 is not proof of the original raw indices. `jiff-tzdb` 0.1.9 exposes
bundled TZif bytes and Jiff can construct a zone from supplied bytes, so a
future source-coupled owner is possible to investigate. It is not yet proven
equivalent to the actual `TimeZone::get` instance: system/TZDIR database
features can be enabled through feature unification, and public `Zone::Rules`
and `TemporalSession.zone` permit externally constructed zones. Forcing a
bundled source would be a separate compatibility change, not a silent fix.

The actual SQL cast route is `cast_expr` → `casts::scalar` → `cast_scalar` →
`temporal::cast` → `ExtendedType::cast_from_in` → `temporal_types::parse_text`
→ the relevant temporal `parse_*` → `decode_datetime`. Query controls select
the temporal session but are not passed to the tokenizer or abbreviation
resolver. The branch's `decode_datetime_checked` and reachable-interval
classifier are test-only. `tokens::tokenize` currently collects input into a
`Vec<char>` and builds token Strings and a Vec without a checked owner or
interruption. Implicit temporal operands, functions, `source_series`, Bind,
`time_input`, public no-controls callers and errors require their own routing
audit. A resolver callback or a checked transition loop alone is not an
end-to-end control or diagnostic proof.

The focused Jev gate selected this precise punt at 0.95, with raw/API 0.93,
SQL/control 0.87 and approximation 0.81 gap leads. After the integrated typed
POSIX path and the actual bundled raw-byte API were included, the follow-up
selected the precise punt at 1.00, with source selection 0.89 and
timing/ownership 0.92 gap leads. Receipts are
`target/assembly-review/815-temporal-input-abbreviation-{request,response}.json`
and `target/assembly-review/815-temporal-input-abbreviation-followup-{request,response}.json`.
Jev selects a direction; it does not establish correctness or impossibility.

Before activation, specify how the exact raw bytes and zone are constructed
and retained together, with a compatible public API and explicit source policy
for bundled, system/TZDIR, POSIX and externally supplied zones. Prove raw
paired-designation-index and orphan-type classification, final-date resolution,
session/global/keyword and error precedence, same-owner lifetimes and the
fallible SQL/public handoffs. Add actual-work synchronized cancellation,
exact/below-budget, diagnostic and teardown probes with no new arbitrary cap.
The paired-index and orphan TZif fixtures and checked-decoder probes remain
test-only prototypes; their oracle observations are not Cassie execution
evidence. The selected MSK SQL regressions are independent execution evidence,
not proof for those broader paths. #815 remains open.
