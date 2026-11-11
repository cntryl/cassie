# Selected transactional DDL publication qualification (#833)

## Scope

This record qualifies the shared atomic-publication boundary for the finite
#826/#839 statement inventory in the [pgwire transaction contract](pgwire-transaction-contract.md#finite-ddl-statements-from-826-and-839).
The current source baseline was `origin/main` at `22a8324dc9110d1b0b56dbc3eaa8ee8f0498caef`.

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

| Gate | Result |
|---|---|
| `cargo build --locked --bin cassie` | Passed |
| `cargo test --locked` | Passed: 11,699 passed, 0 failed, 12 ignored across 40 test binaries |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | Passed |
| `cargo fmt --all -- --check` | Passed |
| `cntryl-tools validate-tests -f tests/compatibility/lifecycle/sequences/truncate_identity_rollback.rs` | Passed: 4/4 tests compliant |

## Remaining acceptance

The selected DDL publication boundary is qualified on this source baseline.
Keep #833, #826, #839, and #819 open until their owning acceptance is reconciled
against the full finite statement matrix. Row-conversion pairs, remaining
clone/constraint lifecycle edges, transaction-owned sequence lifecycle beyond
the tested create/default/`nextval`/savepoint cases, broader #819 transaction
wait rules, and full catalog tuple MVCC are not claimed complete here.
