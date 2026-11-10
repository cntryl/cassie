# Controlled JSON existence function handoff

For `jsonb_exists_any` and `jsonb_exists_all`, preserve the existing arity,
SQL NULL, scratch admission, document conversion and key conversion order. If
statement controls exist, pass the converted operands to the reviewed checked
existence operator, keeping the existing inspection scratch reservation alive.
Use the legacy operator when the checked path returns no supported candidate;
callers without controls retain the existing route. Boolean results need no
result lease. Private Value-mode sentinel behavior remains unchanged.

Jev selected this scoped reuse (0.74); argument/NULL/parse/fallback priority
(0.71), overlapping scratch/arena/path owners (0.72) and sentinel/operator
scope (0.64) require the authored differential and synchronized probes.
Receipt `target/assembly-review/json-exists-function-contract-{request,response}.json`.
Additional arena and path admission can reject earlier, and checked work may
observe cancellation before the old result. Existing argument conversion and
cloning, the scratch estimate as a physical bound, fallback reparsing,
allocation transients and teardown remain unqualified. This does not resolve
the plain-sentinel/storage migration or complete #809/#810; probes and all
Cargo checks remain deferred to the consolidated gate.
