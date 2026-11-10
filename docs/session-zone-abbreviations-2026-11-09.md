# Session-zone abbreviation rollback

The operand-time abbreviation search introduced for #815 is withdrawn. PostgreSQL
18.6 treats `AAA0AAA-1,M3.2.0,M11.1.0` as two duplicate raw designations and
recognizes the first `AAA` designation as fixed UTC. For a July operand the
operand-time transition search sees +01 instead. The old resolver returns UTC
when the transaction start is in winter. Both timestamp and timestamptz
`timezone` overloads have authored, unexecuted frozen-clock regression probes.

The old resolver is restored exactly for all zones. A POSIX-only guard would
not cover a named custom or system TZif with duplicate designation strings; an
IANA name does not establish the source bytes or rule out that case. Scanning
the currently bundled TZDB found no such duplicate designation, but downstream
feature unification can use a different Jiff database. Preserved isolated WIP
contains the desired historical IANA and global-precedence acceptance probes.
The historical-abbreviation, cancellation and latency limitations of the old
resolver remain open; this rollback does not complete #815.

Jev selected full restoration over an unproved POSIX-only guard
(`target/assembly-review/timezone-duplicate-regression-contract-*`). The
PostgreSQL 18.6 oracle was stopped after confirming the behavior. No Cassie
build or test has been run for this change.
