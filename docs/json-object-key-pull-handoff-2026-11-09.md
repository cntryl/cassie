# Guarded object-key expansion in pull cursors

The generated-source pull path has two consumers of its private `projected`
helper: a derived `QueryCursor` stores a flat row iterator between calls, and
the outer cursor loops over projected rows while applying OFFSET and LIMIT.
`apply_projection_phase` now returns `OwnedExpanded`, so the previous bare
`Vec<Batch>` signature no longer matches. Return the guard from `projected`,
wrap the existing EXISTS path as untracked, and consume batches using a private
owning iterator. Its current row iterator and remaining batch iterator precede
the chunk reservation in field drop order.
The derived cursor retains the iterator across calls; the outer cursor retains
it across its offset/limit loop and early breaks. Output order, filtering and
pagination remain in their original callers. Jev selected this streaming route
(0.92; ownership and cancellation lead 0.85), receipt
`target/assembly-review/source-pull-chunk-repair-{request,response}.json`.

The generated output Vec and Arc payload boundaries remain unqualified. No new
production cancellation check is introduced by the iterator. Tests cover outer
and nested generated paths, empty results, LIMIT/OFFSET, partially consumed
iterator drop, and actual row-move cancellation; all remain unexecuted.
