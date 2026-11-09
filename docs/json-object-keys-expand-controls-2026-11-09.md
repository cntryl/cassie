# Checked JSON object-key row expansion

For the existing single bare JSON object-key projection, borrow its output name
from the projection. For each key, pre-admit and copy the name and key in
checked UTF-8 chunks and pre-admit the one-entry row directory. Keep the
prepared row before its reservation on all failure paths. Insert the prepared
row into the existing `AccountedVec` with a same-tracker guard, charge its
row slot, and immediately absorb the prepared lease after insertion. The source
row and its owner remain live through the copy; existing row order, NULL and
non-text errors are preserved. A cancellation check after the completed transfer
can now win before the next row or final return.

Jev selected this scope (0.88); owner transfer (0.82) and ordering (0.90)
leads require actual name/key copy, prepared-directory/slot failure, same and
foreign tracker, exact/below overlap and last-clone probes. Receipt:
`target/assembly-review/json-object-keys-projection-{request,response}.json`.
The newly admitted row directory can change budget outcomes. The preexisting
AccountedVec backing-capacity convention, Arc/header and chunk directories,
allocator transient excess, input/source construction and teardown remain
unqualified; do not treat this increment as complete projection accounting.
Tests and Cargo checks remain deferred to the consolidated gate.
