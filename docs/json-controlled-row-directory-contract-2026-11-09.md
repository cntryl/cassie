# Controlled row directory for raw JSON cells

This is an implementation proposal for #809, not a runtime or qualification
claim. It builds on the test-only inline raw-cell candidate and preserves the
existing TYPE_JSON persisted tag and direct Midge byte boundary. The current
`row_blob.rs::RowDirectory::parse` owns an unchecked HashMap and a Vec sorted
with `sort_unstable`; a borrowed-field shortcut would not qualify their peak
allocation, interruption or lifetime.

The existing parser checks magic, format version, schema/max-field metadata
and bitmap lengths in source order, then reads all directory entries into a
HashMap. Duplicate field IDs overwrite earlier entries. Only after all
entries are read does it validate surviving offset/length bounds, sort their
ranges and require a contiguous exact partition of payload bytes. A lookup
then checks presence and SQL-null bitmaps before requiring a directory entry
and verifying the tag against schema. In a declared JSON/JSONB column a
legacy generic SQL-null bit yields no JSON value, while a TYPE_JSON field
containing bytes `null` is a present JSON null. Projection and alias handling
are later consumers and must retain their current behavior. Any replacement
must not introduce extra corruption checks or change those decisions.

A proposed controlled parser first reads every encoded entry into an
admitted directory, then performs a checked stable sort by field ID and keeps
the last encoded entry of each duplicate group. It checks all surviving
offsets and bounds, then uses a separately admitted checked sort of ranges to
enforce the same partition before making any field available. All old and new
buffers must remain coupled to reservations through replacement and errors;
the source row must outlive borrowed field spans and remain held until any
owned cell copy completes. Fallible admission and interruption may precede
later corruption errors and must be recorded. Fixed-width integer comparisons
still need checked loops and buffer movement; no depth/size cap is selected.

The HashMap iteration order is nondeterministic, but all existing invalid
offset or overflow branches emit the same field-bounds error. A source audit
and differential cases must verify this remains true for duplicate IDs,
overlapping, gapped, truncated, unselected and trailing payload ranges,
SQL-null bitmap/tag mismatches, unsupported versions and invalid UTF-8. Exact
error class/message and relevant priority must be preserved except the
documented new control failures. Probes must exercise actual sort/move
cancellation, old/new buffer overlap, exact/below budgets, duplicate-ID last
selection, field owner/drop order, SQL NULL versus JSON null and old row bytes.
No unproved serde Value, row-blob public compatibility, storage recovery,
column-store, index, function or Midge adapter claim follows from this design.
The current public Value/serde shape and Midge API remain unchanged.

Jev selected the stable ID/coalesce then lexicographic range-sort direction
(0.96). Its duplicate-overwrite (0.87), zero/equal-start and partition
precedence (0.56), owner (0.32) and resource (0.80) leads require focused
probes and source review. Receipt:
`target/assembly-review/controlled-row-directory-contract-{request,response}.json`.
Bounds are checked only after duplicate coalescing; tuple range order and the
distinct partition versus trailing-byte errors remain unchanged.

The isolated prototype also hands the borrowed TYPE_JSON bytes to the
test-only inline plain cell before the row is released. UTF-8 validation and
copying occur in checked chunks; JSON preserves raw text and JSONB formats via
the plain arena. A later test-only error handoff retains directory/control
errors and uses legacy diagnostics only if legacy decoding also fails; its
legacy scratch and remaining plain-error diagnostics are unqualified. The
plain parser's existing nesting limit and row-source lifetime integration
remain pending. It is not a production decoder or a storage migration.
