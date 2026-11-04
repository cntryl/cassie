# Character array cast qualification

This source-only #796/#807 increment implements the selected
[character array conversion law](type-contract.md#character-array-element-conversion-2026-11-04).
It is based on combined query parent `94042b92`, including array/source
corrections `12e1889d`. It changes no Midge engine, dependency version, row
encoding, storage layout or public value representation.

## Behavior and ownership

Native arrays map selected scalar elements through the existing checked scalar
cast policy, retaining dimensions, lower bounds and NULL positions. Unknown
array literals parse without a target character length before explicit casts
truncate by character count and pad CHAR. Source CHAR converted to TEXT or
VARCHAR drops insignificant trailing ASCII blanks. Bounded CHAR also uses the
existing source-aware TEXT formatter before applying width, preserving the
statement's temporal settings for scalar and array elements.

Array descriptors inherit element modifiers for explicit casts, constructors
and typed subquery/subscript expressions. Ordinary function, aggregate and
concatenation results retain their base array identity with typmod -1. The
existing UNNEST scalar descriptor law remains unchanged.

Character input validation rejects non-space overflow with `22001`; only
excess ASCII spaces may be clipped. Input inspection validates without
materializing padding. Resource, deadline and cancellation errors propagate
instead of becoming a false validity result or an error-info row. The private
array input error keeps its SQLSTATE through scalar casts, wire input and CSV
input. General implicit assignment and array-cast parity are not claimed.

The converter admits parsing, native formatting, output slots and padding
before growth. It reuses `EvaluationMemory` for generated arguments and VALUES,
returns the same owner with semantic constants, and retains a narrow owner on
ordinary projection rows containing character-array casts. Cancellation is
checked between elements and while adding padding. Original row owners and
projection search, session and subquery resolver context remain intact.

The stored canonical CHAR spelling is unchanged. Explicit recasts restore
padding from declared type metadata. Direct stored CHAR-array rendering and
UNNEST without a cast still require their separate logical-value qualification;
this increment does not claim that wider #807 acceptance cell complete.

## Evidence and pending validation

The finite pinned PostgreSQL18.6 receipt contains 21 queries: 17 successful
results and four intentional `22001` assignment failures. It records explicit
truncation/padding, multibyte character boundaries, lower bounds, NULL/empty
distinctions, CHAR-to-text conversion, input validation, native INT/BOOL
correspondence and array OIDs/typmods. These are oracle receipts, not Cassie
test results or a PostgreSQL parity claim.

Nine wire tests, seven source-local unit tests and one SQL resource test are
authored and **unexecuted**. They include text/binary parameters and results,
multidimensional negative bounds, statement timezone correspondence, ordinary
function result modifiers, stored canonical recasts, assignment rejection and
same-session recovery. Admission probes use just-below/at-bound budgets and an
element conversion counter. Cancellation is triggered at a private synchronous
between-element hook. Constant and projection witnesses retain padding until
their owner drops. Native NUMERIC scale is admitted before formatting despite
a small final character width.

The existing `CHAR(1000000)[]` VALUES resource witness now requires
`ResourceLimit` instead of the former staged `Unsupported` rejection. Its
assertion does not accept unpadded success. A computed array projection uses
lowered budgets to distinguish denial from admission, then checks owner release
and a follow-up query.

Jev selected native element mapping, existing scalar correspondence and context
owners at 1.00, and separate input/explicit modes at 0.99. Initial review gaps
were preallocation 0.32, projection handoff candidate 0.39 and binary shape
candidate 0.30; each has a focused authored probe above. The final review
retained memory escape 0.48 and native semantics 0.56 while selecting integration
at 0.68. These probabilities remain unresolved leads until the full gate runs;
they are not reduced by another review. The array function descriptor correction
uses pinned `exprTypmod` source and Jev's selected normalization law (0.81).

Per the user's deferred-run instruction, no Cargo build, Cassie test, Clippy or
test-policy command ran for this increment. Rustfmt parsed/formatted the touched
Rust files, and `git diff --check` passed. The complete ordered validation and
expanded oracle comparisons belong to the frozen combined bundle. No issue
closure, hosted CI, merge or support promotion is claimed here.

Raw primary sources, oracle output and Jev requests/responses are preserved in
the isolated worktree's ignored `target/v020-evidence/character-arrays/` without
credentials.
