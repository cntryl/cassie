# Mode String result ownership

The #806 mode input vector is now value-first under its two reservations, but
the winning `Value` is cloned without a result owner and returned bare. The
selected increment handles a winning `Value::String` only. Jev selected this
scoped path (1.00; gap lead 0.67), receipt
`target/assembly-review/806-mode-string-owner-{request,response}.json`.

Stable sorting and the existing strict `count > best_count` rule retain the
first representative in the chosen ASC/DESC order. Store its sorted index for
String candidates; preserve existing update-time cloning for all non-String
candidates, including mixed carrier sequences. The selected final String is
copied once in checked UTF-8 chunks into exact pre-admitted capacity while both
input reservations remain live. Its value-first sibling reservation moves into
the existing private aggregate output and row owner. A String-only private
constructor retains a zero-byte lease for an empty result, so its alias/row
handoff is also admitted. Check interruption within each equal-key run.

This moves String allocation and admission to the final winner, and permits a
resource or cancellation error there before output handoff; a new tie-run check
may preempt later work. Prior intermediate String clones are no longer made.
Non-String best-candidate cloning and its error order stay on the old path.
Source tests must cover both tie directions and first representative, a strict
winner, mixed carrier updates, NULL/FILTER/empty inputs, empty String ownership,
actual UTF-8 and tie-run cancellation, exact/below input/result/copy overlap,
foreign trackers, aliases and last-row-clone lifetime. They are unexecuted.
The pinned PostgreSQL 18.6 oracle returned `a`/`b` for ASC/DESC over
`[b, a, b, a, NULL]`, `b` for `[a, a, b, b, b]`, and an empty String for
`[empty, empty, a]`. Cassie's timestamp-spelling normalization is a separate
existing comparator behavior; its first-representative witness is not a claim
that PostgreSQL TEXT compares those distinct spellings equal.

Input expression conversion, `compare_values` and its allocation/cancellation,
non-String result ownership, pre-existing group payloads, allocator transients
and total synchronous teardown remain separate unqualified boundaries. This
increment does not complete #806.
