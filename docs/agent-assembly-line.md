# Coder, review and assembler

This assembly line delivers one coordinator-defined batch through three roles.
The default is one coder, one reviewer and one assembler, plus the primary
coordinator. The project agents inherit the parent's model and permissions.
Their instructions allocate ownership; they are not separate security sandboxes.

## Roles and ownership

| Role | Owns | Handoff |
| --- | --- | --- |
| `coder` | Focused regression first, then smallest implementation and owning contract docs, in assigned paths | Immutable commit/range, base and tree; test and acceptance mapping; actual evidence status |
| `review` | Source/oracle review and focused Jev questions; independent defect verification | Exact reviewed base/head/tree; questions/results; defect evidence and dispositions |
| `assembler` | One integration worktree/branch, shared registration/schema/composition/inventory, local gate and one combined PR | Integrated review, ordered gate receipts, final-head hosted status and merge readback |

The coordinator chooses the highest-priority available slice from the current
GitHub backlog and canonical docs, verifies dependencies, declares file ownership
and batch acceptance, and handles scope changes. Independent coders may be added
only with disjoint ownership and an explicit coordinator allocation. Keep one
assembler and one compiler owner. Do not spawn a recursive agent tree.

## Batch protocol

1. Define the batch before coding. Copy the handoff template to the integration
   worktree's ignored `target/assembly-line/<batch>/manifest.md`. Record exact
   issues, acceptance criteria, base, dependencies, owners, worktrees and limits.
   Preserve existing dirty work and refs; never reset or clean them away.
2. Coder authors focused `should_` regressions with Arrange / Act / Assert, then
   implements the selected contract. Compilation and test execution stay deferred
   until the substantial combined bundle is frozen. Mark unexecuted probes
   `authored/not run`; do not claim an unobserved RED.
3. Review follows the real functions and callers, sibling paths and explicit
   SQL cases. Use the live issue evidence and native semantic oracle where
   applicable, then submit concrete semantic choices or adversarial questions to
   Jev. Keep credentials private and requests/responses under ignored `target/`.
   A gap probability >= 0.30 requires a focused probe. Source and executable
   evidence decide whether a finding is real; Jev is not proof.
4. Defects return to coder as evidence plus a focused regression requirement.
   Record fixed, disproved, blocked or linked-follow-up dispositions. A changed
   commit/tree invalidates its prior review. Review the new immutable source.
5. Assembler collects related reviewed commits in dependency order. It owns
   conflict resolution and shared integration edits. Before compilation, freeze
   the integrated head/tree and have review examine the complete combined diff
   and cross-change interactions. Input reviews alone do not qualify integration.
6. Run the complete local gate once the batch is coherent, in repository order:

   ```sh
   cargo build --locked --bin cassie
   cargo test --locked
   cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic
   cargo fmt --all -- --check
   cntryl-tools validate-tests -f <each-touched-test-file>
   ```

   Update/check generated inventories. Use private build artifacts in that
   worktree. On an observed failure, return the defect, freeze the repair,
   obtain affected review, and repeat affected checks. Never narrow strict
   Clippy, suppress lints, weaken assertions, skip suites or relabel cancellation.
7. Assembler publishes one PR for the complete batch after local qualification.
   Record exact validation commands and outcomes with the required PR sections.
   Normal push/PR workflows remain enabled. Wait for actual final-head checks
   and review readback before an authorized squash merge. Verify the merged
   commit/tree and issue closures. No per-coder PR or tiny workflow-push loop.

Use isolated coder worktrees for parallel implementation. A single-coder pilot
may use the integration worktree directly with disjoint file ownership; its
commit is still reviewed and frozen before the assembler starts gates. Copy-on-
write artifact copies are acceptable; sharing a mutable target directory is not.

## Manifest states

`defined -> coding -> review -> changes_requested | accepted -> assembling ->
integrated_review -> frozen -> validating -> locally_qualified -> published ->
hosted_qualified -> merged`

Every transition records its owner, UTC time and source identity. A source edit
from `accepted` onward returns affected work to `review`; integration edits also
require `integrated_review`. A failed gate is a failure, a running gate remains
running, and a stopped gate is cancelled. A dependency blocker does not count as
completion. At a user timebox, stop new work and preserve the current state,
logs and unfinished checks for process review; do not manufacture a completed
batch to fit the timebox.

## Permanent boundaries

Use published `cntryl-midge` directly as the only storage layer. No engine edits,
second abstraction or invented API. An unavailable Midge capability must punt
with its exact blocker and next action recorded. Keep PostgreSQL wire primary,
REST secondary, and the finite product scope. No CI/release/version configuration
edits without explicit current-request authorization. Memory fixtures are the
default; disk tests retain their real persistence/restart/layout assertions.

## Starting a batch

Use a prompt such as:

> Run the Cassie assembly line for this documented slice: <issues and scope>.
> Assign `coder` bounded paths, have `review` use Jev and source/oracle evidence,
> and have `assembler` combine the changes into one PR and run the full local
> gate only after the integrated bundle is frozen. Timebox: <deadline or none>.
> Report the manifest state and exact unfinished checks at the review point.

Agent definitions are in `.codex/agents/`. The coordinator remains responsible
for launching the roles, delivering handoffs and enforcing the declared scope.
