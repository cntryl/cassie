# Scheduled Retention Operator Guide

Scheduled retention is driven by an external UTC scheduler over Cassie's
administrative PostgreSQL wire connection. Cassie stores schedule progress and
executes one bounded batch per due call; it does not create a background
worker. A due call scans at most 256 row IDs in stable order and commits row
deletes, referential effects, and progress together.

## Configure a daily policy

Create the policy as usual, then set its first UTC due instant. The start must
be UTC midnight. Each successful daily window advances `next_due_at` by one day.

```sql
CREATE RETENTION POLICY events_retention
  ON public.events USING event_at RETAIN FOR '30 days';

ALTER RETENTION POLICY events_retention
  SCHEDULE DAILY START AT '2026-01-02T00:00:00Z';
```

Connect as the administrative `root` role to schedule, suspend, resume, or run
scheduled enforcement. Configure the scheduler with an admin-only DSN and keep
its credential in the scheduler's secret store.

## Run one bounded tick each minute

The external scheduler compares UTC now with the persisted `next_due_at` and
passes that exact due instant as `DUE AT`. This keeps the cutoff for a missed
schedule occurrence distinct from the time when the external process noticed
it. Cassie starts the oldest due window; when a window is pending, each later
tick advances at most one batch in that same window. If several daily runs were
missed, the scheduler keeps passing the oldest `next_due_at` until that window
completes, then catches up the next day on a later tick.

```sh
#!/usr/bin/env bash
set -euo pipefail

: "${CASSIE_ADMIN_DSN:?set an administrative PostgreSQL wire DSN}"

while :; do
  now="$(date -u +'%Y-%m-%dT%H:%M:%SZ')"
  due_at="$(psql "$CASSIE_ADMIN_DSN" -X -At -v ON_ERROR_STOP=1 \
    -c "SELECT next_due_at::TEXT FROM pg_catalog.pg_retention_policies WHERE policy_name = 'events_retention' AND schedule_state = 'active' AND next_due_at <= '$now'::TIMESTAMPTZ")"
  if [[ -n "$due_at" ]]; then
    psql "$CASSIE_ADMIN_DSN" -X -v ON_ERROR_STOP=1 \
      -c "ENFORCE RETENTION POLICY events_retention DUE AT '$due_at'"
  fi
  sleep 60
done
```

The loop is intentionally outside Cassie. Run only one tick worker per policy
unless the external scheduler provides overlap control. Cassie's collection
gates serialize overlapping calls on one node, so a duplicate call observes
the committed cursor and does not double-delete rows.

Inspect schedule and window state with:

```sql
SELECT policy_name, schedule_state, next_due_at, pending_cutoff,
       pending_scanned_rows, pending_deleted_rows, pending_skipped_rows,
       last_successful_cutoff, last_deleted_rows, last_skipped_rows,
       last_failure, last_error
FROM pg_catalog.pg_retention_policies
WHERE policy_name = 'events_retention';
```

`pending_cutoff` is the original `DUE AT` instant, not the row deletion
boundary. Rows are eligible when `event_at < pending_cutoff - retention_duration`.
Progress remains pending until the cursor reaches the end of the relation. A
full 256-row page stays pending without reading a 257th row, so a relation with
exactly 256 rows remaining takes a following tick to confirm the end; that call
commits only the completed policy metadata. The completed counters then move to
`last_deleted_rows` and `last_skipped_rows`, `last_successful_cutoff` advances,
and `next_due_at` moves one day forward.

## Suspend, replay, and resume

Suspending the schedule makes future `DUE AT` ticks no-ops and preserves pending
progress. If a batch stopped after a committed cursor advance, use explicit
`AT` to finish the pending window, even while suspended. The pending window's
stored enforcement instant takes precedence over the timestamp supplied for
replay.

```sql
ALTER RETENTION POLICY events_retention SUSPEND;

-- Check pending_cutoff and pending_* counters before replay.
ENFORCE RETENTION POLICY events_retention AT '2026-01-02T00:00:00Z';

ALTER RETENTION POLICY events_retention RESUME;
```

The replay retains the original cutoff and resumes after the saved row ID. It
does not advance `last_successful_cutoff` until the entire window completes. A
later row correction behind the saved cursor is picked up by the next daily
window. `last_failure` retains the most recent scheduled failure after a
successful retry; `last_error` keeps its existing clear-on-success behavior.

Keep the progress row and maintenance diagnostics with operational incident
records. Do not advance `next_due_at` manually; retry or resume the persisted
window so row and cursor state remain aligned.
