# JSONB numeric producer boundary qualification

This source candidate repairs the confirmed #809 producer boundary after the
[relational semantic-key candidate](jsonb-semantic-key-qualification.md).
Executable qualification remains pending under the current compilation deferral.

The existing numeric format permits 131072 integer digits, display scale up to
16383, and exponent magnitude up to 1073741823. Leading coefficient zeroes do not
consume integer weight. PostgreSQL 18.6 accepts `0.01e131073` and rejects
`0.01e131074` with `22003`. Zero is exempt from integer-weight growth, but keeps
its display-scale and exponent limits: `0e1073741823` renders `0`, while the next
exponent and `0.00e-16382` overflow.

A private borrowed decimal-shape helper counts significant digits without an
allocation. Generic NUMERIC input validates grammar first, then checks this shape
before constructing the new decimal coefficient or a power of ten. SQL radix,
separator, typmod and display-scale behavior remain on their existing paths.
JSONB output-size estimation and private semantic-key normalization share the
same format checks. Canonical byte bounds account for scale, the integer zero,
negative nonzero values and the decimal point without expanding an exponent.

The output estimator validates local JSON numeric-token grammar. A suffix such
as `1e131072x` therefore remains `22P02`; a valid token outside the format reports
`22003` before requesting expanded-output memory. This remains a sequential
private size estimator, not a second whole-document parser. Existing canonical
JSON parsing and compound malformed-document error phases are unchanged.

The regressions are authored and have not run:

- [NUMERIC input](../src/types/extended/numeric_decimal_tests.rs) and
  [JSONB size/canonical boundaries](../src/types/jsonb_numeric_tests.rs), including
  a 24-case PostgreSQL spelling corpus across sign, zero, scale and weight.
- [Actual construction admission](../src/executor/json_construction/numeric_boundary_tests.rs):
  measured just-at/just-below output admission and true format errors before an
  output reservation, through the shared bound/reserve/encode/finish path.
- [SQL values](../tests/sql_queries/types/numeric/jsonb_numeric_boundaries.rs)
  and [wire errors/recovery](../tests/pgwire_extended/types/json/numeric_weight_boundaries.rs).

Ignored evidence lives in `target/v020-evidence/jsonb-numeric-producer/`.
Jev selected the shared borrowed shape and the existing-contract interpretation
with probability 1.00. Its focused review assigned range risk 0.33 and an
under-bound candidate 0.32; the independent 24-case pinned spelling corpus and
corresponding native probes were added in response. These remain model leads
and authored probes, not passing Cassie runtime evidence. The final combined
ordered build/test/strict Clippy/format/test-policy and resource qualification
must pass before this cell is closed.
