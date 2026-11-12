# Aggregate cancellation fixture scope

Issue #954 requires an instance-scoped retained-input boundary. The previous
diagnostic hook installed a thread-local pause, so any subsequently created
query handle on that thread could take it, including a handle from another
Cassie instance or an unrelated backend. That behavior could make a wire
fixture synchronize with the wrong request.

The existing doc-hidden hook now takes the Cassie instance and the target
pgwire backend's process ID and secret. The pending pause belongs to that
instance's backend registry, is claimed only by a new query on that backend,
and follows the query handle's clones once claimed. Dropping the guard removes
only its own unclaimed pause; disconnecting a backend removes its pending
entry. Unit-only direct attachment supports operator tests. This changes the
signature of an existing public, doc-hidden diagnostic function and can require
updates for downstream Rust callers of that function; it adds no new public
pause function. The existing sleep and regex hooks retain their own scopes.
An aggregate pause and a sleep or regex pause may coexist on one query handle;
each retains a separate one-shot owner and reaches only its own boundary.

The authored probes cover two instances and two backends on one executor,
an intervening bare handle, invalid and duplicate identities, guard drop,
disconnect, and real CancelRequest with no output and recovery. They have not
been run on this candidate. Additional probes cover coexistence with a sleep
pause, cloned-handle one-shot behavior, and an old guard dropped after a new
pause is armed. The rest of #954, including its full ordered gate
and timing comparison, remains open.

## Active regex cancellation source review — 2026-11-11

The live #954 acceptance requires a small synchronized active-regex cancellation
probe after prerequisite #799. `gh issue view 799` reports `CLOSED`, closed at
2026-11-06T00:29:02Z. The historical `should_cancel_recursive_regex_work_with_cleanup`
million-row/sleep race is absent from
`tests/planner_controls/cancellation/query_cancellation.rs`. Its current
replacement already exists as
`tests/sql_queries/patterns/pattern_matching/cancellation.rs::should_cancel_active_regex_work_before_output_publication`.

The replacement uses a three-row recursive query and the pgwire helper in
`tests/support/regex_cancellation.rs`. The helper arms a one-shot diagnostic
pause, waits for the actual regex search to complete and report `true`, sends a
real backend `CancelRequest`, then resumes execution. The execution path in
`src/executor/filter/regex_functions.rs::regexp_like_text` checks cancellation
after matching and before returning the boolean, so the test exercises active
query cancellation at the real matcher boundary before SQL result publication.
The doc-hidden hook is request-owned once claimed by its query handle; it adds
no production pause setting. The regression asserts SQLSTATE 57014, no
DataRow/CommandComplete, idle ReadyForQuery, accounted-memory return to the
pre-query value, and same-connection retry. A sibling test covers the private
ARE constraint path.

A focused Jev review asked whether this post-search boundary satisfies the
active issue criterion or cancellation must pause inside the matcher. Jev
1.14.0 selected `post_search_boundary_satisfies` (0.94 probability, 0.91
confidence), versus `must_pause_inside_matcher` at 0.05 and `other_specific_gap`
at 0.01. The redacted request and response are under
`target/v020-evidence/954/regex-review.request.redacted.json` and
`target/v020-evidence/954/regex-review.response.redacted.json`. This is advisory
source review, not proof. The targeted regex cancellation tests and ordered
bundle gates were not run on the current consolidated head in this lane; prior
test records do not qualify this exact head. No code or regression change was
needed because the current source already contains the bounded synchronized
replacement. Broader #954 acceptance and coordinator qualification remain
open.
