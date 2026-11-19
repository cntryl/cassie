# Time-Series Calendar Dashboards

Use the existing timezone-aware SQL functions to build local calendar dashboards. This is a
prepared query contract; it does not add calendar-specific syntax or stored index metadata. A
fixed-UTC time-series index may accelerate the event-range branches, while the exact query remains
the correctness path. The templates assume `events.device` is `text`, `events.event_at` is
`timestamptz`, and `events.amount` is `int`.

## Bind contract and boundaries

Both templates take seven parameters in this order:

| Parameter | Type / OID | Meaning |
| --- | --- | --- |
| `$1` | `text` / 25 | First device ID |
| `$2` | `text` / 25 | Second device ID |
| `$3` | `timestamptz` / 1184 | Inclusive UTC event start |
| `$4` | `timestamptz` / 1184 | Exclusive UTC event end |
| `$5` | `timestamptz` / 1184 | Inclusive calendar-grid start, aligned to a local boundary |
| `$6` | `timestamptz` / 1184 | Exclusive calendar-grid end, aligned to a local boundary |
| `$7` | `text` / 25 | Explicit IANA time zone, such as `America/New_York` or `UTC` |

`$1` and `$2` must identify distinct devices. The query uses `UNION ALL` for both input and grid
branches, so equal IDs would double the matching observations and duplicate every calendar row.

The event range and calendar grid are independent. Both event predicates are half-open UTC
instants, `event_at >= $3 AND event_at < $4`; a row whose `event_at` is NULL cannot match. Grid
start and end must be instants for local day or month boundaries in `$7`. The four-argument,
typed `generate_series` advances in that zone's calendar. Since its stop value is inclusive, the
templates filter out `$6` to produce an exclusive end. `$5` is the calendar-grid origin; each
subsequent boundary is derived by advancing from it in the selected calendar. The timezone-aware
series signature and its inclusive stop behavior are documented in
[Set-returning sources](set-returning-sources.md).

The result columns are `device text`, `local_label date`, `boundary_utc timestamp without time
zone`, `observation_count bigint`, `total bigint`, and `carried_total bigint` (result OIDs
`25, 1082, 1114, 20, 20, 20` for the example `amount int`). OID 1114 has no time-zone metadata;
`boundary_utc` contains the UTC wall-clock representation of the same instant as the calendar
boundary. It is stable when the connection's session time zone changes. Rows are ordered by
device, then UTC boundary.

## Prepared daily query

This query can be sent through PostgreSQL's SQL `PREPARE` or the pgwire extended-query protocol.
The type list makes the parameter OIDs explicit.

```sql
PREPARE dashboard_day (
    text, text, timestamptz, timestamptz, timestamptz, timestamptz, text
) AS
WITH
device_a AS MATERIALIZED (
    SELECT device, event_at, amount FROM events
    WHERE device = $1 AND event_at >= $3 AND event_at < $4
),
device_b AS MATERIALIZED (
    SELECT device, event_at, amount FROM events
    WHERE device = $2 AND event_at >= $3 AND event_at < $4
),
bucket_totals AS (
    SELECT device, date_trunc('day', event_at, $7::text) AS boundary,
           count(*) AS observations, sum(amount) AS total
    FROM (SELECT * FROM device_a UNION ALL SELECT * FROM device_b) input
    GROUP BY device, date_trunc('day', event_at, $7::text)
),
grid AS (
    SELECT $1::text AS device, boundary
    FROM generate_series(
        $5::timestamptz, $6::timestamptz, interval '1 day', $7::text
    ) AS s(boundary)
    WHERE boundary < $6
    UNION ALL
    SELECT $2::text AS device, boundary
    FROM generate_series(
        $5::timestamptz, $6::timestamptz, interval '1 day', $7::text
    ) AS s(boundary)
    WHERE boundary < $6
),
joined AS (
    SELECT grid.device, grid.boundary,
           bucket_totals.observations, bucket_totals.total,
           sum(CASE WHEN bucket_totals.observations IS NULL
                    THEN 0::bigint ELSE 1::bigint END)
               OVER (PARTITION BY grid.device ORDER BY grid.boundary) AS run_id
    FROM grid
    LEFT JOIN bucket_totals
      ON bucket_totals.device = grid.device
     AND bucket_totals.boundary = grid.boundary
),
carried AS (
    SELECT device,
           (boundary AT TIME ZONE $7::text)::date AS local_label,
           boundary AT TIME ZONE 'UTC' AS boundary_utc,
           coalesce(observations, 0)::bigint AS observation_count,
           total,
           first_value(total) OVER (
               PARTITION BY device, run_id ORDER BY boundary
               ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
           ) AS carried_total
    FROM joined
)
SELECT device, local_label, boundary_utc,
       observation_count, total, carried_total
FROM carried
ORDER BY device, boundary_utc;
```

Example New York day-grid binds, with the event window ending before the final displayed day:

```sql
EXECUTE dashboard_day(
    'device-a', 'device-b',
    '2024-03-08 06:00:00+00', '2024-03-12 14:00:00+00',
    '2024-03-08 05:00:00+00', '2024-03-13 04:00:00+00',
    'America/New_York'
);
```

The UTC grid boundaries are `2024-03-08 05:00:00`, `2024-03-09 05:00:00`,
`2024-03-10 05:00:00`, `2024-03-11 04:00:00`, and `2024-03-12 04:00:00`. The spring-transition
day is 23 hours. During the fall transition, adjacent local-day boundaries span 25 hours; the two
distinct UTC instants that both display as local 01:30 remain distinct input events.

## Prepared monthly query

The monthly template uses the same bind and result contract. The grid boundaries must be the
first instant of a local month in `$7`; it returns each local month once, with its UTC start.

```sql
PREPARE dashboard_month (
    text, text, timestamptz, timestamptz, timestamptz, timestamptz, text
) AS
WITH
device_a AS MATERIALIZED (
    SELECT device, event_at, amount FROM events
    WHERE device = $1 AND event_at >= $3 AND event_at < $4
),
device_b AS MATERIALIZED (
    SELECT device, event_at, amount FROM events
    WHERE device = $2 AND event_at >= $3 AND event_at < $4
),
bucket_totals AS (
    SELECT device, date_trunc('month', event_at, $7::text) AS boundary,
           count(*) AS observations, sum(amount) AS total
    FROM (SELECT * FROM device_a UNION ALL SELECT * FROM device_b) input
    GROUP BY device, date_trunc('month', event_at, $7::text)
),
grid AS (
    SELECT $1::text AS device, boundary
    FROM generate_series(
        $5::timestamptz, $6::timestamptz, interval '1 month', $7::text
    ) AS s(boundary)
    WHERE boundary < $6
    UNION ALL
    SELECT $2::text AS device, boundary
    FROM generate_series(
        $5::timestamptz, $6::timestamptz, interval '1 month', $7::text
    ) AS s(boundary)
    WHERE boundary < $6
),
joined AS (
    SELECT grid.device, grid.boundary,
           bucket_totals.observations, bucket_totals.total,
           sum(CASE WHEN bucket_totals.observations IS NULL
                    THEN 0::bigint ELSE 1::bigint END)
               OVER (PARTITION BY grid.device ORDER BY grid.boundary) AS run_id
    FROM grid
    LEFT JOIN bucket_totals
      ON bucket_totals.device = grid.device
     AND bucket_totals.boundary = grid.boundary
),
carried AS (
    SELECT device,
           (boundary AT TIME ZONE $7::text)::date AS local_label,
           boundary AT TIME ZONE 'UTC' AS boundary_utc,
           coalesce(observations, 0)::bigint AS observation_count,
           total,
           first_value(total) OVER (
               PARTITION BY device, run_id ORDER BY boundary
               ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
           ) AS carried_total
    FROM joined
)
SELECT device, local_label, boundary_utc,
       observation_count, total, carried_total
FROM carried
ORDER BY device, boundary_utc;
```

For example, use event bounds `2024-01-01 06:00:00+00` through exclusive
`2024-03-31 22:00:00+00`, and a New York grid from `2024-01-01 05:00:00+00` through exclusive
`2024-04-01 04:00:00+00`. The output month boundaries are January 1, February 1, and March 1
at `05:00:00` UTC. Events exactly at either event bound follow the half-open predicate: the
inclusive start is included, and the exclusive end is not.

```sql
EXECUTE dashboard_month(
    'device-a', 'device-b',
    '2024-01-01 06:00:00+00', '2024-03-31 22:00:00+00',
    '2024-01-01 05:00:00+00', '2024-04-01 04:00:00+00',
    'America/New_York'
);
```

## Empty buckets, NULL amounts, and limits

The left join emits each requested device/boundary pair. An empty bucket has `observation_count =
0` and `total = NULL`. `count(*)` treats an event with a NULL amount as an observation, while
`sum(amount)` remains NULL if all amounts in that bucket are NULL. The cumulative observation
count starts a new carry run for every observed bucket; `first_value` carries that bucket's sum
only within the same device and run. Leading gaps therefore stay NULL, later gaps carry the most
recent observed sum, an observed all-NULL bucket resets the carry to NULL, and one device's value
cannot carry into another device.

The result grid contains one row per device per included boundary. Bound the number of devices and
the requested interval so this product stays within Cassie's configured
`CassieRuntimeConfig.limits.max_result_rows` (default 100,000 rows). Exceeding the result-row
limit returns SQLSTATE `54000`; the prepared pgwire contract returns no partial `DataRow` frames.
The server setting `CASSIE_MAX_RESULT_ROWS` controls this cap. Normal query memory and deadline
limits (`CASSIE_QUERY_MEMORY_BUDGET_BYTES` and `CASSIE_QUERY_TIMEOUT_MS`) also apply. Calendar
queries do not bypass those limits.

For indexed reads, keep each device's direct half-open timestamp predicate in its own materialized
branch, as shown above. A fixed-UTC index such as
`CREATE INDEX events_by_time ON events USING time_series (event_at) WITH (bucket_width = '1 day', partition_by = device)`
may accelerate the input filter; the `date_trunc` grouping and local grid remain query behavior,
not index metadata. Exact and indexed results must match.
