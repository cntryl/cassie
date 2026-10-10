# Portable kernel qualification

This record covers the finite sliced-selection composition selected by
[#889](https://github.com/cntryl/cassie/issues/889), a child of
[#774](https://github.com/cntryl/cassie/issues/774) under
[#747](https://github.com/cntryl/cassie/issues/747). It adds a private test for
existing behavior. The broader `PIPE-08`, `VEX-20` and `VEX-21` obligations remain
with open parent #774. This record grants no support promotion or SIMD
implementation/performance claim.

## Pinned scope and dispatch

Source baseline: `55b52e26ac570554ba394ec06d45568853e2866d`.
The [typed batch contract](typed-batch-contract.md) selects ordered repeated
positions, composed Slice/Gather mappings, TRUE-only filtering, exact descriptors
and retained admission through the last consumer. The architecture proposal body
has SHA-256 `b28dea1b7e0103563fad1391dfe231bee91e3fee09b43dbdcf5bb81a934e5e85`;
its handoff has SHA-256
`980d1cea1f333e92a8caf98f23fb7903758ed3234486d7d93c4bfb6d9651d8b3`.
The live #889 body has SHA-256
`01405922603966af1da29f71215a5b514dd398d9fae47852d5fded97e7549a5e`;
its only differences from the proposal are removal of the proposed-title line
and addition of #888 to the current PR inventory. The mechanical contract is
unchanged.
The implementation follows that finite contract without a public API, persistent
format, storage, runtime budget or optional ISA change.

| Owner | Selected routing |
| --- | --- |
| `src/executor/typed_batch.rs`: `TypedBatch::slice` | Unselected input slices each column; selected input slices its ordered position map. |
| `src/executor/typed_batch/column.rs`: `slice`, `gather`, `cell` | Immutable views retain parents and compose offsets/maps, including validity. |
| `src/executor/typed_batch/capability.rs`: `expression` | The selected BIGINT comparison and Boolean AND must return exactly `NativeTyped`. |
| `src/executor/typed_batch/operations.rs`: `filter`, `native_value` | Actual helper uses the native numeric/Boolean path and keeps only TRUE lanes, preserving ordered repetitions. |
| `src/executor/typed_batch/operations.rs`: `project`, `project_column` | Named passthrough columns become admitted gathers and rebase the output to identity selection. |
| `src/executor/filter.rs`: `evaluate_expr_value` | Independent scalar predicate evaluates `BatchRow`s constructed directly from original fixture rows. |

## Finite witness

The single test is
`executor::typed_batch::tests::sliced_selection::should_preserve_sliced_selection_through_native_filter_projection`
in [sliced_selection.rs](../src/executor/typed_batch/tests/sliced_selection.rs).

| Original row | n: BIGINT | flag: BOOLEAN | payload: TEXT |
| --- | ---: | --- | --- |
| 0 | 100 | TRUE | left |
| 1 | 10 | TRUE | a |
| 2 | NULL | NULL | β |
| 3 | 30 | FALSE | c |
| 4 | 40 | TRUE | λ |
| 5 | 999 | TRUE | right |

The test constructs `base` with domain six and identity selection, calls
`base.slice(1, 4)`, attaches selection `[0,3,3,1,2]` to those actual sliced
column views with `from_views`, then calls `selected.slice(1, 4)`. The final
window must contain original rows `[4,4,2,3]` in that order. Exact logical
lengths, schema and every window value are asserted before filtering. Both
boundary sentinels and the removed first selection lane qualify, making an
ignored offset or ignored selected-map slice observable.

Predicate `n >= 0 AND flag` must be `NativeTyped` before the actual `filter`
helper is called. The scalar oracle reads the original column fixture at the
literal indices `[4,4,2,3]`; it does not read a typed position, cell or value.
Its expected truths are TRUE, TRUE, NULL and FALSE. Projection
`n AS x, n AS y, payload AS p, payload AS q` must have ordered schema
`(x BIGINT, y BIGINT, p TEXT, q TEXT)` and exactly two copies of
`(40,40,'λ','λ')`, compared with both the scalar output and explicit rows.

All base/intermediate batch handles are dropped before projected values are
read. The remaining batch has a positive current memory charge. Cloning it
must leave that charge unchanged; dropping the first handle must retain the
same charge and exact schema/values through the last alias. Dropping that alias
must release all accounted memory. The assertions use measured current charges,
without a fixed byte budget or architecture-dependent owner-size literal.
This private view test launches no workers and supplies no global worker
measurement.

Neighboring repeated-selection, sliced-column lifetime and 1025-row boundary
tests remain separate controls. This witness does not introduce a type, batch
size, tail/mask or ISA grid.

## Evidence and execution receipt

The test was written before execution or production change. Initial qualification
is **GREEN** on unchanged production baseline: the new focused test passed on
its first execution, and no runtime RED was observed. A failure would have been
retained and classified before runtime edits or broader scope were proposed.

| Receipt | Status |
| --- | --- |
| Mechanical contract and existing helper routing inspected | Source review complete; baseline pinned above. |
| Architecture Jev composition-gap probability 0.92 | Two-slice/native/scalar/owner probe executed and passed; see focused receipt. |
| Focused unchanged-baseline probe | **PASS**: 1 passed, 797 filtered, zero failures on the pinned source baseline plus this test. |
| Jev draft source review | `jev-1.13.0` favored handoff 0.96, confidence 0.95; details below. No execution certified. |
| Independent review of routing, oracle and lifetime witness | Source mapping, direct scalar oracle and retained-owner assertions reviewed with no concrete gap; review did not certify execution or full acceptance. |
| Full local validation | **PENDING**: the ordered full gate still requires the shared #880 baseline repair to land; focused execution, formatting and touched-file validators are recorded above/below. |
| Final-head hosted checks, review, squash merge and issue readback | **PENDING** integration owner. |

Jev was asked which concrete source action the supplied draft supported, whether
its original-row oracle had a concrete correlation/expected-value omission, and
whether its post-parent-drop lifetime/accounting assertions had a concrete gap.
The request supplied the pinned proposal, test and current dispatch/view
owners. The returned judgments motivated the exact focused execution. The source-action distribution was
handoff 0.96, oracle correction 0.01, routing correction 0.01, lifetime correction
0.02 and none 0.00. Oracle-gap probability was 0.18; lifetime-gap probability
was 0.28. The actual exact test passed, addressing the selected composition witness. Full
local validation and final-head hosted/review evidence remain **PENDING**; Jev
judgments do not prove correctness.

Ignored evidence files under `target/qualification-889/` retain the exact request
and response. Request `draft-review-jev-request.json` has SHA-256
`f7a2b929b766fa624d0e28fe1327d8441553a5fa2d66189ead7fe18c54ebaa95`;
response `draft-review-jev-response.json` has SHA-256
`1f79bd74cd7fbf86271e97796402bdbdfd489ebd2c6c4cf7e5a32c7126ab2e61`.

Focused baseline command, executed on the pinned source baseline:

```sh
cargo test --locked --lib executor::typed_batch::tests::sliced_selection::should_preserve_sliced_selection_through_native_filter_projection -- --nocapture
```

Required final validation remains, in order:

```sh
cargo build --locked
cargo test --locked
cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic
cargo fmt --all -- --check
cntryl-tools validate-tests -f src/executor/typed_batch/tests/sliced_selection.rs
cntryl-tools validate-tests -f src/executor/typed_batch/tests.rs
```

The focused baseline command passed 1 test with 797 filtered. `cargo fmt --all -- --check`, both touched-test validators, `cntryl-tools validate-docs --config .cntryl/repository.toml`, module-size check and `git diff --check` passed. The ordered full build/test/Clippy gate remains pending and must be rerun on the final head after the #880 baseline PR is merged. No execution receipt from another issue or worktree satisfies these gates.
Parent #774 remains open for its separate required qualification and any later
measured optional SIMD decision.
