# BatchRow operator-owner destruction

`BatchRow` now declares its private operator-memory owner after its row-owned
fields. Rust drops fields in declaration order, so ordinary destruction frees
values, aliases, lookup and outer scope before the last operator reservation can
be released. This changes private layout and destruction timing, not the row's
public construction or query behavior. A clone/outer-parent/last-owner release
probe is authored but unexecuted; it verifies owner lifetime, while the actual
field ordering is established by source and Rust's field-drop rule.

This does not qualify consuming handoffs. `into_entries` and `into_values`
move bare values out while row reservation fields drop; each production caller
must separately retain or admit the moved allocations. Cloned values likewise
need their own admission. The source/Jev decision is recorded in
`target/assembly-review/batchrow-drop-order-{request,response}.json` in the
coordinator worktree. No tests or build have run for this candidate.
