# Finite typed join implementation plan

Issue [#759](https://github.com/cntryl/cassie/issues/759) follows completed #756.
The coherent bundle selects INNER and LEFT, one pure column equality, numeric
SMALLINT/INT/BIGINT/FLOAT (including mixed widths) and BOOLEAN keys. NULL keys
never match. Exact numeric equivalence comes from the existing semantic key
owner, including signed zero, exact integral floats and admitted nonfinite values.

Build/probe consume complete common TypedBatch inputs, retaining all payload
columns. Hash values are row positions with a preallocated duplicate chain;
output consists of bounded typed gather/dictionary batches with separate outer
validity. Input compatibility conversion and the admitted joined-row consumer
handoff are explicit boundaries of the existing source execution API. Payload
transport preserves every supported logical family and original row aliases and
metadata; it is not a SIMD or zero-copy scan claim.

The loaded-source adapter uses admitted collection catalog schemas and homogeneous
row names/aliases. Private loaded callers can supply complete declared row types.
Empty loaded inputs or missing declared domains, nonnumeric/nonboolean keys, residual and
composite predicates, RIGHT/FULL/CROSS, lateral/apply and indexed/merge sources
retain the existing legal scalar/indexed/merge paths. A typed empty-side batch
is supported by the private kernel; source shape fallback preserves complete
outer extensions when the loaded API lacks typed empty metadata. Existing
bounded source/side selection and pre-output adaptive replay remain unchanged.

Reservations precede full input conversion, hash/chain construction, match maps,
typed output views and row handoff. Controls are checked during build, probe and
every hot-key expansion. Final admitted rows can remain materialized under the
existing query budget; no spilling or streaming portal lifetime is introduced.

Acceptance requires red/green differential witnesses for duplicate/NULL/empty
keys, outer shape, signed zero and exact mixed numerics, quoted column identity,
selection repetition, typed payload preservation, capped fanout, denied admission
and deterministic mid-build/probe cancellation. Existing residual, nullable
indexed keys, legality/replay fixtures remain the fallback oracles. Every assigned
PIPE-05/EXEC-10..13/VEX-10..11 owner must receive an executed witness or explicit
selected boundary before closure. Final ordered and hosted validation remain
pending; this record claims no completed issue closure.

## Local witnesses and invariant disposition

- PIPE-05: the selected kernel accepts full common typed batches; all 17 logical
  payload families survive input destruction. Collection source conversion and
  final admitted row consumers are explicit compatibility boundaries.
- EXEC-10: typed empty-right output preserves complete schema and nullable outer
  payload; SQL empty-loaded input uses the existing shape-aware fallback.
- EXEC-11: pure selected equality uses native typed keys. Residual ON predicates,
  nullable indexed alternatives and bounded source selection retain existing
  legal paths; the differential residual witness includes a late matching row.
- EXEC-12: quoted Boolean identity and qualified input aliases are retained;
  explicit schema ownership takes precedence over shared relation suffixes.
  Existing reorder, outer and lateral barriers remain authoritative; this slice
  introduces no optimizer reorder rule.
- EXEC-13: existing pre-output adaptive selection and replay own switching;
  native success diagnostics publish only after final output/control handoff.
  Loaded left/right input totals are separate from inserted non-NULL build keys
  and processed probe rows; a one-output cap witnesses 3/3 inputs and 2/1 work.
- VEX-10: duplicate and NULL keys, exact mixed numerics beyond 2^53, signed-zero
  payload bits, repeated selections and typed outer payload have local witnesses.
- VEX-11: a fixed admitted build chain replaces per-key growing groups; bounded
  match maps check controls for every hot-key lane. Denied build admission,
  deterministic build cancellation and fanout cancellation release reservations.

Focused kernel and SQL tests are green. Ordered repository validation, exact-head
hosted checks and review remain pending. Unsupported joins and key families above
remain fallbacks; no all-joins, SIMD, spill or portal streaming claim is made.

## Iteration evidence

Baseline: `4a09c41a3670dad3a4c4c2b7ef7583327c37ab6b`. Dependency #756 is closed.
The initial typed payload witness failed against a not-implemented kernel, then
all seven typed kernel witnesses passed. Native SQL path assertions separately
failed while ordinary loaded rows lacked complete transport domains; admitted
catalog schema transport fixed that boundary. The qualifier ownership witness
then failed on reversed operands across identically named tables in two schemas;
first-present-candidate ownership fixed it.

Focused commands use `CARGO_BUILD_JOBS=2` and the isolated worktree's target:

```sh
cargo test --locked --lib typed_batch::join::tests:: -- --nocapture
cargo test --locked --lib source_join:: -- --nocapture
cargo test --locked --test sql_queries join -- --nocapture
```

Results: 7/7 typed kernel tests, 39/39 source join controls/retention tests and
32/32 SQL join tests passed. The cross-schema SQL pair executed the existing
bounded `vectorized` owner and the ordered loaded `typed_hash` owner with the
same hand-derived row. The SQL run also executed the existing merge NULL-key,
indexed numeric equality, signed-zero and adaptive replay witnesses.

Jev's preallocated-chain concern (0.58) led to a fixed admitted duplicate chain.
The outer payload concern (0.38) led to executed staged ARRAY/JSON outer SQL and
all-family retained-owner probes. The remaining qualifier concern (0.43) led to
the cross-schema owner test and paired bounded/loaded SQL path probes. These are
targeted evidence, not substitutes for the pending repository validation gates.

The typed diagnostic roles regression first failed with one left input reported
for three loaded rows. The private roles carrier now records actual loaded input
counts separately from kernel work, without new public metrics or changes to the
legacy vectorized reporter.

Jev flagged capped/exhausted probe accounting (0.48). A focused loaded typed
probe with NULL and unmatched prefix/tail rows checks budgets 1, 2 and 3:
actual inputs remain 4/3, non-NULL build work is 2, and processed probe work
is respectively 3, 3 and 4. This verifies early cap, exact fanout cap and
exhausted unmatched-tail accounting.

A repeated roles judgment (0.32) led to all-NULL key probes: the INNER path
loads 3/3 inputs, inserts zero build keys and exhausts all three probe rows;
LEFT caps of 1 and 3 process respectively one and three outer probe rows.
All probes preserve the final output and return query memory to zero.

Final scenario-specific Jev review selected handoff (0.65; remaining probe
0.12, fix 0.14). Full ordered repository gates and exact integrated revision
qualification remain the parent delivery queue obligations.
