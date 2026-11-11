# Selected transactional DDL publication qualification (#833)

## Scope

This record qualifies the shared atomic-publication boundary for the finite
#826/#839 statement inventory in the [pgwire transaction contract](pgwire-transaction-contract.md#finite-ddl-statements-from-826-and-839).
The current source baseline is `origin/main` at
`3b81ccea55cdc9e60023bf02cf41607ca1904c11` (#1199); this worktree rebases the
prior local #833 evidence commit onto that source before the current gate.

Cassie stages and publishes through `cntryl-midge` 0.5.2 as its only storage
layer. The selected transaction remains within one Midge database family. This
record does not select full PostgreSQL catalog tuple MVCC, compatibility or
migration between unreleased builds, cross-database atomicity, or a second
storage layer.

## Dependency and semantic decision

Issue #819 remains open for broader transaction conflict/wait semantics and
PostgreSQL catalog tuple visibility. The source on this baseline already
contains the selected #833 prerequisites used here: retained REPEATABLE READ
data/catalog owners, old-plus-own metadata composition, stale catalog-write
preflight, and same-family row/catalog publication. The broader #819 acceptance
is not required to qualify this bounded publication path and is not closed by
this record.

Jev's typed choice selected `A_include_focused_atomicity_regression`
(probability 0.91, confidence 0.86) after reviewing the live #833/#826/#839
scope and current source. The choice was to add end-to-end evidence for
transaction-owned SERIAL/IDENTITY defaults and `nextval`, then change runtime
source only if that evidence failed. Jev is a scope judgment, not correctness
proof. The request and response are retained in ignored
`target/jev/issue833-sequence-boundary-request.json` and
`target/jev/issue833-sequence-boundary-response.json` files.

## Source and regression evidence

`src/app/transaction_semantics.rs::transactional_ddl_supported` admits the
selected table, schema, index, and materialized-view DDL families.
`src/app/query_transactions.rs::commit_transaction_inner` publishes the
staged Midge storage overlay and catalog snapshot through the same transaction
commit boundary. Staged sequence advancement uses the transaction-private
sequence owner in `src/midge/adapter/transactions/sequence_owner.rs`; no Midge
source or dependency change was needed.

The new pgwire compatibility regressions in
[`truncate_identity_rollback.rs`](../tests/compatibility/lifecycle/sequences/truncate_identity_rollback.rs)
prove:

- A standalone identity-table Query returns after both its table and sequence
  are visible to a second session; that session can insert through the default.
- In an explicit transaction, SERIAL and IDENTITY sequence metadata and rows
  remain private before commit, `nextval` and generated defaults work in the
  owner, and the observer sees the complete table/sequence/row state after
  commit.
- Savepoint rollback removes a later identity table, sequence, and row while
  preserving earlier staged DDL and its row.

Existing focused evidence covers the remaining shared boundaries:

- Explicit DDL visibility, rollback, and savepoints:
  [`transactional_ddl.rs`](../tests/sql_mutations/transactions/transactional_ddl.rs)
  and [`ddl_extensions/transactions.rs`](../tests/compatibility/lifecycle/ddl/ddl_extensions/transactions.rs).
- Commit/restart and rollback readback:
  [`restart_publication.rs`](../tests/sql_mutations/transactions/restart_publication.rs)
  and [`atomic_lifecycle.rs`](../tests/compatibility/lifecycle/ddl/atomic_lifecycle.rs).
- Standalone publication admission/cancellation and mixed Simple Query
  rejection: [`query_implicit_ddl/tests.rs`](../src/app/query_implicit_ddl/tests.rs)
  and [`transactions.rs`](../tests/compatibility/lifecycle/ddl/ddl_extensions/transactions.rs).
- Materialized refresh cancellation and recovery:
  [`materialized_view_concurrent_tests.rs`](../src/executor/execution/materialized_view_concurrent_tests.rs).

These are publication-boundary tests. They do not establish every independent
row-conversion, clone-option, constraint, or lifecycle obligation in #826/#839.

## Statement-by-statement acceptance matrix

The live #833 body selects the exact finite forms in the transaction contract;
the live #826 and #839 bodies still own their SQL semantics, conversion pairs,
clone metadata, and dependency behavior. `Proven` below means a retained test
exercises the named publication/lifecycle boundary. It does not transfer the
downstream issue's full feature acceptance to #833. Existing tests were included
in the prior full gate on pre-rebase commit `39e02d45`; the new LIKE regression
ran in the current ordered gate recorded below.

| Selected statement forms | Implicit standalone Query/Sync | Explicit transaction | Row/catalog atomicity and rollback/savepoint | Cancellation and restart | Deterministic rejection |
|---|---|---|---|---|---|
| **#826 — `CREATE TABLE`** with selected defaults, `GENERATED ALWAYS` / `GENERATED BY DEFAULT AS IDENTITY`, `SERIAL`, and named constraints | Proven for Simple Query publication: `query_implicit_ddl/tests.rs::should_publish_implicit_table_creation_before_returning_its_command`; identity/sequence observer readback: `truncate_identity_rollback.rs::should_publish_implicit_identity_table_with_its_sequence_at_query_completion`. | Proven: `truncate_identity_rollback.rs::should_publish_transaction_owned_generation_sequences_with_rows` stages SERIAL/IDENTITY sequence use and rows, then observes complete state after commit. | Proven: `truncate_identity_rollback.rs::should_restore_transaction_owned_identity_sequence_after_savepoint_rollback`; generic table/row rollback and savepoint: `transactional_ddl.rs::should_restore_transactional_catalog_at_savepoint_rollback` and `ddl_extensions/transactions.rs::should_atomically_rollback_table_creation_after_insert`. | Implicit publication-lock cancellation is proven by `query_implicit_ddl/tests.rs::should_cancel_an_implicit_ddl_publication_wait_before_staging`; committed table/alter/row restart is proven by `restart_publication.rs::should_preserve_transactional_catalog_publication_across_restart`. These are shared-boundary witnesses, not cancellation/restart tests for each declaration form. | Unselected DDL is rejected in explicit transactions by `tests/sql_mutations/transactions/transaction_semantics.rs::should_reject_unselected_ddl_in_active_transaction`; unsupported table forms reject before target creation in `table_forms/create_table_as.rs` and `table_forms/semantics.rs`. Mixed Simple Query rejection is covered by `ddl_extensions/transactions.rs::should_reject_mixed_simple_query_ddl_before_execution`. |
| **#826 — `CREATE TABLE ... LIKE`** with selected `INCLUDING DEFAULTS`, `CONSTRAINTS`, `IDENTITY`, `INDEXES`, and `ALL` combinations | Proven for Simple Query: `table_forms/like_options.rs::should_apply_like_all_with_explicit_exclusions`, `should_copy_scalar_indexes_when_like_includes_indexes`, and `sequences/ddl_extensions/semantics.rs::should_copy_identity_sequence_independently_with_like_including_identity`. | Proven on the current tree: `table_forms/transactions.rs::should_restore_like_identity_publication_to_savepoint` stages target-owned identity and copied catalog metadata with a row, proves observer isolation, rolls back the savepoint, recreates the same name, and publishes the replacement. | Explicit clone/savepoint and row/catalog atomicity are directly checked by the new test. No clone-specific local-restart witness is present in the current source. | Generic CREATE TABLE admission cancellation is covered above; there is no LIKE-specific cancellation or restart witness. Those per-form lifecycle cells remain with #826. | Unsupported LIKE options and checks reject before target creation in `table_forms/semantics.rs::should_reject_table_like_before_creating_the_target` and its `IF NOT EXISTS` counterpart; cloned supported options have positive witnesses linked at left. |
| **#826 — `CREATE TABLE AS`** over `SELECT`, `TABLE`, `VALUES`, or prepared `EXECUTE`, selected column lists, `USING heap`, `WITH DATA` and `WITH NO DATA` | Proven for Simple Query by `table_forms/create_table_as.rs`, `table_forms/metadata.rs`, `table_forms/parameters.rs`, `ddl_extensions/semantics.rs`, and `tables/lifecycle/semantics.rs`; failed materialization leaves no catalog row in `create_table_as.rs::should_rollback_ctas_table_when_query_materialization_fails`. | Proven for rollback/read-your-writes/observer visibility by `create_table_as.rs::should_rollback_explicit_transaction_ctas_for_all_sessions`; nested CTAS ownership is also exercised by `query_implicit_ddl/tests.rs::should_keep_internal_table_creation_owned_by_ctas`. | Full explicit rollback and failed-build atomicity are proven. No CTAS-specific savepoint test is present; generic DDL savepoint coverage does not assert the distinct CTAS executor path. | No CTAS-specific cancellation or local-restart publication witness is present. Those cells remain open under #826; ordinary-table restart evidence is not treated as CTAS evidence. | Unsupported CTAS storage choice rejects without creating a relation in `create_table_as.rs::should_reject_unsupported_ctas_access_methods_without_creating_a_table`; mixed Simple Query is rejected before execution as above. |
| **#826 — selected SQL materialized-view `CREATE`, `REFRESH`, rename, and `DROP`** | Simple Query lifecycle is covered by `tables/lifecycle/semantics.rs`, `tables/materialized_view_create_no_data.rs`, and `materialized_views/mutation_ddl/semantics.rs`. | Explicit create/refresh commit or rollback is covered by `transactional_ddl.rs::{should_publish_materialized_view_after_commit, should_rollback_materialized_view_publication_with_transaction, should_rollback_materialized_view_refresh}`; rename commit/rollback is covered by `ddl/materialized_view_rename.rs`. | Cross-session visibility, output rows, and rollback are directly checked by the named transaction tests; rename rollback is checked in `materialized_view_rename.rs`. No separate savepoint witness is claimed. | Restart is proven by `restart_publication.rs::should_recover_persisted_materialized_view_after_restart`. Refresh cancellation cleanup and restart are proven by `materialized_view_concurrent_tests.rs::{should_cleanup_unpublished_indexes_when_concurrent_refresh_is_canceled_before_activation, should_recover_previous_output_after_canceled_concurrent_refresh_restart}`. | Failed view build cleanup is covered by `transactional_ddl.rs::should_remove_failed_materialized_view_creation`; unsupported dependent rewrites are rejected before rename in `ddl/materialized_view_rename.rs::should_reject_unsupported_dependent_definitions_before_materialized_rename`. |
| **#839 — `ALTER TABLE`** selected `ADD COLUMN IF NOT EXISTS`, `ALTER COLUMN TYPE ... USING`, multi-action, column/table rename, `SET SCHEMA`, and `DROP COLUMN CASCADE` / `RESTRICT` | Simple Query ALTER forms are covered by `ddl_extensions/semantics.rs`, `atomic_lifecycle.rs`, and `drop_column_cascade.rs`; these are operation-level tests, not a complete protocol cross-product. | Explicit transaction row/catalog staging is covered by `ddl_extensions/transactions.rs` and `transactional_ddl.rs`; multi-action failure recovery uses `ddl_extensions/transactions.rs::should_rollback_multi_action_alter_to_savepoint_after_constraint_failure`. | Transaction rollback/savepoint is directly covered by the named transaction suite; conversion failure leaves rows/metadata unchanged in `atomic_lifecycle.rs::should_rollback_multi_action_alter_after_using_failure`; restart is covered by `should_recover_multi_action_alter_after_restart`, `should_keep_unique_enforcement_after_using_type_rewrite_restart`, and `drop_column_cascade.rs::should_recover_column_cascade_dependency_removal_after_local_restart`. | The shared implicit publication-wait cancellation witness applies to the common owner path; no cancellation test is specific to each ALTER form. Restart evidence exists for selected rewrite/cascade cases, not all selected conversions. | Invalid multi-action constraint changes and `RESTRICT` dependency cases reject without partial mutation in `ddl_extensions/semantics.rs`, `atomic_lifecycle.rs`, and `drop_column_cascade.rs`. |
| **#839 — `DROP TABLE`, `DROP VIEW`, and `DROP SCHEMA`** selected name lists, `IF EXISTS`, `CASCADE`, and `RESTRICT` | Simple Query success and all-target preflight are covered by `atomic_drop.rs`, `ddl_extensions/semantics.rs`, and `if_exists_notices.rs`. | Explicit rollback is covered by `atomic_drop.rs::should_restore_every_multi_drop_target_after_explicit_rollback`; table-drop rollback with staged data is covered by `transactional_ddl.rs::should_restore_dropped_table_after_transaction_rollback`. | Multi-target rejection does not partially drop earlier names in `atomic_drop.rs`; table/foreign-key cascade rollback and restart are covered by `foreign_key_ddl_lifecycle.rs`. No drop-specific cancellation or savepoint case is claimed. | Restart is proven for selected cascade/dependent cleanup by `foreign_key_ddl_lifecycle.rs::should_preserve_inbound_foreign_key_removal_after_table_cascade_restart` and `drop_column_cascade.rs`; cancellation has only the shared implicit admission witness. | Missing later names and `RESTRICT` dependencies reject before partial drop in `atomic_drop.rs` and `ddl_extensions/semantics.rs`; absent/present `IF EXISTS` command behavior is covered for Simple Query and extended protocol by `if_exists_notices.rs`. |
| **#839 — primary-key, unique, check, and foreign-key constraint lifecycle** | Selected creation/drop and validation are exercised in compatibility lifecycle, mutation, and constraint suites; named constraint error visibility is covered by `foreign_key_ddl_lifecycle.rs`. | Explicit DDL staging and failed-constraint recovery are covered by `ddl_extensions/transactions.rs` and `transactional_ddl.rs`; FK cascade rollback has a direct witness in `foreign_key_ddl_lifecycle.rs::should_restore_inbound_foreign_key_after_table_cascade_rollback`. | Constraint DDL rollback/restart is directly covered by `foreign_key_ddl_lifecycle.rs`, `drop_constraint.rs`, and `composite_unique_constraints.rs`; the full constraint/type/dependency combination matrix remains #839 acceptance. | No per-constraint cancellation witness is present; cancellation evidence is shared admission plus materialized refresh. Constraint/index retained state after restart has focused witnesses in the listed suites. | Duplicate, missing-parent, dependent-drop, and unsupported constraint forms fail deterministically in the constraint lifecycle suites; they do not authorize broader PostgreSQL constraints. |

### Shared Midge-family invariant and remaining matrix gaps

All matrix rows use one `Cassie` Midge adapter and one transaction-selected
database. `src/app/query_transactions.rs::commit_transaction_inner` captures
one `(database, storage_overlay)` pair, passes it to row publication, and only
then publishes the matching catalog snapshot; `src/app/transaction_semantics.rs`
admits the supported DDL forms into that owner. The code has no cross-database
transaction publication path, and this qualification makes no such claim. The
new LIKE regression uses two wire sessions to that same configured database.

The new passing `LIKE` regression closes the explicit clone/savepoint gap found
in this audit. The matrix still leaves statement-specific CTAS savepoint,
cancellation, and restart evidence open, and does not establish
extended-protocol execution for every listed statement variant. Those are not
silently counted from generic table tests; resolve them against the owning
#826/#839 acceptance before treating those issues as done. The shared
cancellation and restart boundary is proven only where linked above.

## PostgreSQL 18.6 oracle

The isolated local oracle reported `PostgreSQL 18.6 (Homebrew)` on
`aarch64-apple-darwin25.6.0`.

- In REPEATABLE READ, CTAS using `information_schema.tables` to count its own
  target returned `0` in the created table; the next command counted `1`.
- In one transaction, SERIAL `nextval` returned `1`, the following SERIAL
  default returned `2`, and a BIGINT IDENTITY default returned `1`. The owner
  saw four table/sequence catalog rows before rollback; all four were absent
  after rollback.

The Cassie regressions retain these selected values and visibility boundaries.
The oracle does not select stale-base catalog tuple-mask behavior; that remains
outside this #833 slice under #819/#833.

## Ordered local gates

These results apply to the current source tree rebased onto
`3b81ccea55cdc9e60023bf02cf41607ca1904c11` and include the LIKE regression.
An initial full-suite attempt overlapped another worktree's Cargo tests and
timed out in one unrelated pgwire test; that test then passed alone (1/1), and
the uncontended full suite passed.

| Gate | Result |
|---|---|
| `cargo build --locked --bin cassie` | Passed on rebased source, including the final test structure. |
| `cargo test --locked` | Passed: all configured targets and doc-tests exited 0 on the uncontended final-tree run. |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | Passed on the final test structure. |
| `cargo fmt --all -- --check` | Passed after applying rustfmt to the touched test module. |
| `cntryl-tools validate-tests -f tests/compatibility/lifecycle/tables/table_forms/transactions.rs` | Passed: 1/1 compliant. |
| `cntryl-tools validate-tests -f tests/compatibility/lifecycle/tables/table_forms.rs` | No tests found; this touched file only registers the new module. |

## Remaining acceptance

| #833 acceptance item | Status after source/test matrix |
|---|---|
| Exact #826/#839 statement inventory | Proven and recorded in the transaction contract and live issue body. |
| Atomic catalog + row publication for selected implicit and explicit paths | Partially proven. Common Create Table, CTAS rollback, materialized-view, ALTER/DROP, constraint, and now explicit LIKE/savepoint witnesses pass. CTAS has no direct savepoint witness, and not every selected statement has an extended-protocol witness. |
| Rollback, savepoint, cancellation, restart, deterministic rejection | Partially proven. Shared and selected-form witnesses are linked above. CTAS savepoint/cancellation/restart and per-form cancellation remain open; do not infer them from generic table coverage. |
| Single Midge-family scope; no cross-database or full PostgreSQL parity claim | Proven as the documented and source-backed publication boundary. No cross-database experiment or parity claim is made. |

Keep #833 open for the remaining matrix gaps and final gate evidence. Keep #826,
#839, and #819 open for their own acceptance: row-conversion pairs, remaining
clone/constraint lifecycle edges, transaction-owned sequence lifecycle beyond
the tested create/default/`nextval`/savepoint cases, broader #819 transaction
wait rules, and full catalog tuple MVCC are not claimed complete here.
