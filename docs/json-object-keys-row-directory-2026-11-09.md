# Capacity-coupled JSON object-key output directory

Replace the selected expansion's `AccountedVec` with a dedicated owner whose
row Vec, actual-capacity reservation and independently transferred row-payload
reservation are field-ordered. Grow by admitting a replacement buffer while
the old buffer and all row payload leases remain live, then move and reverse
rows with interruption checks before retiring the old buffer and lease. A
prepared row and its lease stay value-first through tracker checks, growth and
insertion; insertion is followed immediately by the infallible same-tracker
payload absorption. Merge the directory and payload reservations only after
all rows are built, then use the existing source-parent-preserving final
attachment guard. No in-place uninterruptible Vec relocation or logical-length
capacity estimate is used for this selected output directory.

Jev selected parent preservation plus the checked vector (0.59; owner lead
0.87); receipt `target/assembly-review/json-projection-parent-evidence-{request,response}.json`.
The replacement's old/new buffers overlap, and charging actual retained
capacity can change budget outcomes. The shared output lease can retain dead
directory capacity after rows move into chunks, conservatively increasing later
budget pressure. New cancellation checks can precede an old result. Allocator transient excess before capacity inspection, Arc header
deallocation, shared source type admission, lookup and chunk directories,
earlier source/value clones and generic AccountedVec users remain unqualified.
Test-first actual move/reversal cancellation, foreign tracker, failed growth,
partial row/source owner release, exact/below overlap and last-clone probes are
unexecuted until the consolidated gate.
