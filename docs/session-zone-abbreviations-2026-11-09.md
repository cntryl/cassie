# Session-zone historical abbreviations under #815

PostgreSQL 18.6's `DecodeTimezoneAbbrev` checks the session timezone before
its abbreviation catalog. With session timezone `America/New_York`, the
historical abbreviations LMT and EWT still resolve for a 2024 timestamp:
`2024-01-01 00:00:00 LMT` maps to `2024-01-01 04:56:02` UTC, and EWT maps to
`2024-01-01 04:00:00` UTC. The same offsets apply through `AT TIME ZONE`.
The isolated PostgreSQL oracle was stopped after these observations.

Cassie's `Zone::abbreviation_offset` searches at most 64 transitions before
and after the instant, then falls back to its fixed abbreviation catalog. LMT
and EWT are absent from that fallback and predate the 64 most recent New York
transitions, so this cutoff cannot implement the selected contract. The
separate formatted-TZ path already has an interruption-aware full-history
traversal, but normal timestamp input and `timezone`/`AT TIME ZONE` have
different control and error plumbing. A selected implementation must preserve
the public API, PostgreSQL error order and session-zone precedence, propagate
originating controls on SQL paths, and avoid replacing one arbitrary cutoff
with another. The exact normal-input and direct public no-controls boundaries
remain pending until source review and execution. Tests authored for this work
are unexecuted under the consolidated-build deferral.

Jev selected reuse of the full checked search with public-signature
compatibility and separate fallible SQL control propagation
(`target/assembly-review/temporal-historical-abbreviation-contract-*`). This
does not establish cancellation for public or SQL routes that cannot carry
controls. Repeated/ambiguous abbreviations, session/global precedence, unknown
names and cancellation during an actual transition scan require focused source
and oracle probes before claiming this increment complete.

PostgreSQL 18.6 further showed MSK at +04 for a 2012 operand and +03 in 2020,
including both timestamp and timestamptz `timezone` overloads; the 2011 gap
and 2014 fold resolve to 22:30 UTC in the tested local-time cases. Shanghai
CST resolves to +08, while New York falls back to the global -06; Shanghai
LMT is +08:05:43 and New York LMT is -04:56:02. The oracle was stopped.

The narrow draft uses the checked full-history search at the typed operand's
instant for timestamp/timestamptz `timezone` and retains the public `Zone`
shape. Public normal-input behavior and SQL text-input cancellation propagation
are unchanged and still pending. Other
zone-argument callers and global dynamic-abbreviation fallback are not covered.
Unknown names and a first global fixed-name lookup may traverse the complete
Jiff history before fallback, potentially thousands of transitions. Jev
selected a fixed inline per-statement negative bitset only for names in the
existing finite global catalog (`target/assembly-review/temporal-inline-cache-contract-*`).
It is an optimization, not a supported-name limit: cache only a fully completed
miss for the exact immutable session, reset on replacement, and publish under the
same lock after rechecking session identity and cancellation. Repeated global
lookups reuse the miss. The bitset adds a compile-time fixed amount to the
existing controls-slot allocation outside the query tracker. The present catalog
has 77 entries and occupies two u64 words (16 bytes), in addition to any layout
padding; its capacity is computed from the actual catalog lengths at compile time.
The first scan, historical positive lookups, and public no-controls latency remain
explicit qualification limits. The cache never owns a separate allocation.
Existing unknown-zone diagnostic formatting remains separately
unadmitted; no unproved fixed scan limit is introduced.
