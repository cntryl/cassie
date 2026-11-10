# Owned aggregate output copying

For aggregate output rows already carrying query memory, directory growth uses
a separately admitted replacement while the old allocation remains charged.
Entries move with interruption checks and retain their order. Field names and
direct String values use admitted, UTF-8-safe chunked copies. Duplicate names
and row order are unchanged. Newly added checks and overlapping directories
can change cancellation/resource-error precedence or reject tighter budgets.

Allocator excess is detected after allocation and rejected, so transient
allocator over-allocation is not proven admitted. Non-String Value clones and
rows without a query-memory owner retain their previous behavior and are not
qualified by this increment. The existing clone estimates for nested/Typed
values remain separate. Source-owner and cloned-value reservations overlap.

Jev selected this narrow increment; see
`target/assembly-review/json-aggregate-output-response.json`. Tests were
authored before the original draft implementation and remain unexecuted until
the consolidated validation gate.
