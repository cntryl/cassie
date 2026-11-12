# Scalar and calendar qualification increment

This is the next bounded increment in the Boolean/scalar delivery bundle under
#813 and #815, based on merged query/array PR #1192 (`0a49fd8f`). The live
prerequisites #812 (NUMERIC) and #814 (temporal types) are closed. This increment
does not establish completion of either broad function issue.

## Contract selected before implementation

The PostgreSQL 18.6 native oracle rejects formatted `to_date`/`to_timestamp`
inputs with HH24 = 24, seconds = 60, or microseconds = 1000000 with `22008`.
These rules belong to formatted input. Independently supported typed
`TIME '24:00:00'` and `make_time(24,0,0)` remain valid.

The same native oracle requires short MS/US fractions to scale by their input
width (`.1` means one tenth of a second). Delimited MS/US fields read the whole
numeric field before range validation; adjacent fields retain their declared
width when followed by a numeric template field; TZH is a separator for this
purpose in PostgreSQL's pinned `formatting.c`. These finite cases are part of
the repair, without a general date-format compatibility claim. At the time
this increment was authored, FF1–FF6 rounding beyond declared precision was a
separate #815 obligation; its selected contract and evidence are recorded in
[the FF precision increment](temporal-ff-precision-2026-11-06.md).

`to_timestamp(text,text)` must validate its resulting UTC instant after either
an explicit format offset or the session timezone is applied. Conversion must
use checked arithmetic and reject instants outside the existing timestamp range
with `22008`, including overflow near the upper boundary. It must not publish a
row or a successful command for an invalid value. Same-session retry with valid
input must retain the TIMESTAMPTZ descriptor and work normally.

The observed post-zone witness exposed a second part of the same path: a
whitespace template separator was consuming the minus sign before TZH, turning
`-01` into `+01`. Whitespace before a timezone field must preserve its input
sign. Keep generic punctuation handling outside that selected zone boundary.

Jev 1.14.0 selected the scalar/calendar increment with probability 1.00 and the
formatter-specific contract with probability 0.92. The decisions are advisory;
native PostgreSQL output and source define the repair. The native oracle also
disproved a suspected SIGN(NaN) defect: FLOAT8 returns 0 and NUMERIC returns NaN,
matching the existing separate implementations. No SIGN change is needed.

Midge remains the direct published 0.5.2 dependency. This increment needs no new
storage API, durable representation, CI, release, or versioning changes. The
native/background qualification blocker #1188 remains open and is not made
available by this work.

## Evidence and validation

Oracle request/output and Jev receipts are retained locally in the ignored
`target/scalar-calendar-closeout/` directory. New regression tests are authored
before implementation; execution was deferred until the coherent bundle and
Jev review. A five-case standalone SQL witness linked to the unchanged parent's
built library then failed every expected invariant: invalid clock rejection,
excess US rejection, short US scaling, post-zone range rejection, and acceptance
of a wall time moved into range by its offset. This is observed RED evidence;
the permanent SQL/wire regressions' pre-fix failure is not separately claimed.
The same five-case witness passed against the corrected library. The first
complete locked run recorded 10,401 passed, four new formatter regressions
failed, and 12 existing ignores. Those failures supplied executable evidence
for signed-zone and adjacent fraction handling; receipts remain in
`target/scalar-calendar-closeout/attempt-1/`. They are not reported as a pass.

Jev's focused adversarial questions assigned 0.37 to the fraction gap and 0.39
to the timestamp gap. Focused PostgreSQL/source probes confirmed TZH is a
non-digit separator; fraction parsing now distinguishes it from numeric fields.
The range probes covered an offset that overflows INT8, both timestamp ends,
session-zone rejection, and valid offset-restored wall times. The same probes
identified the separate FF precision obligation:
`to_timestamp('2000-01-01 12:00:00.1235', 'YYYY-MM-DD HH24:MI:SS.FF3')` must
return `.124`. That obligation now has a dedicated selected contract,
implementation and focused regression set in
[the FF precision increment](temporal-ff-precision-2026-11-06.md). This
formatter-specific repair remains bounded and does not establish full #815
completion. Jev selected finishing the bounded repair's executable gate with
probability 0.91 after these probes.
Focused review of the signed-zone repair selected the scoped change with
probability 0.86. Complete delimited-field probes also retained significant
digits beyond nine leading zeroes and rejected a large MS integer before
scaling; native PostgreSQL 18.6 confirms these cases. The final field-bound
review selected the repair with probability 0.60; executable evidence remains
the deciding gate.

## Completed local gate

- `cargo build --locked --bin cassie`: passed.
- `cargo test --locked --no-fail-fast`: **10,406 passed, 0 failed, 12 inherited
  ignores**, across 40 completed groups; 494.94 seconds including compilation
  in the warm local worktree. This is not a hosted or release timing claim.
- Strict Clippy initially found one constant `chunks_exact` call in the new
  protocol test. It now uses `as_chunks::<2>()` and asserts the complete batch
  count. The affected prepared-statement regression passed after this test-only
  change; production source was unchanged after the full suite.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`:
  passed after that correction.
- `cargo fmt --all -- --check`: passed.
- `cntryl-tools validate-tests -f <path>`: passed for both new regression files,
  both registration files, and both touched production files (six paths).
- `tools/test_inventory.py --check`: passed with no unregistered tests,
  unresolved modules or duplicate identities.
- `git diff --check`: passed. No ignore, lint suppression, reduced check or
  CI/release/version configuration change was introduced.

Final-head hosted validation is pending. The broader #813/#815 obligations and
#1188 remain open; this increment does not establish complete function,
resource, native-storage or production qualification.
