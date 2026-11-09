# JSON private-sentinel semantics for #809

Status: selected design; implementation and compatibility remain unresolved.
Source baseline `ca4c9077`; this record
is not an implementation or qualification claim. PostgreSQL oracle below used
the locally cached `postgres:18` image, reporting PostgreSQL 18.6. No Cassie
build or test has been run for these observations.

PostgreSQL treats `$serde_json::private::Number` and
`$serde_json::private::RawValue` as ordinary object keys, including when first,
nested, duplicated, or followed by other fields. On PostgreSQL 18.6,
`'{"$serde_json::private::Number":"7"}'::jsonb` remains that object and `?`
for the key returns true; `'{"$serde_json::private::RawValue":"[1]"}'::jsonb`
remains that object; extracting a nested Number-key value `"bad"` yields
`bad`; and a duplicated sentinel key retains its last value. The same oracle
returned true for an escaped-equivalent duplicate key, array string and scalar
string, false for number/null scalars and empty/all-NULL `?|`, and true for
empty/all-NULL `?&`.

Cassie currently has an entry-point and representation mismatch. SQL JSONB
casts call `types::jsonb::canonical`, which deserializes a `serde_json::Value`;
serde 1.0.154 interprets first-key private sentinel names as special Number or
RawValue visitors rather than normal map keys. For example a valid Number `7`
can turn the whole object into a numeric scalar before an operator executes;
an invalid value can instead cause a JSONB input error. The controlled binary
COPY JSONB arena deliberately reproduces that same Value-mode sentinel policy.
The existence implementation likewise deserializes a Value and therefore
cannot restore a key already lost during casting. By contrast SQL JSON's
`cast_json_text` uses `IgnoredAny` for syntax and retains the raw document;
the raw extraction/deletion paths treat sentinel names as ordinary keys.
`Value::Json` and other serde Value round trips can introduce another
interpretation boundary; their reachable producers and consumers require an
explicit audit before any end-to-end compatibility claim.

Confirmed additional boundaries: `src/pgwire/connection/extended/text_parameters.rs`
validates JSONB and then reparses the original text into a serde Value, and
does the same Value parse for JSON; `src/pgwire/connection/codecs/jsonb.rs`
also validates then reparses binary JSONB text. `src/midge/row_blob/encoding.rs`
serializes JSON/JSONB Values into `TYPE_JSON` bytes, while
`src/midge/row_blob.rs` reads those bytes back with `serde_json::from_slice`.
Thus an object constructed correctly before storage can be reinterpreted at
decode. `src/types/semantic/jsonb.rs::JsonbKey::from_text` reparses into Value
for grouping, sorting and joins; JSON equality, order, JSONPATH and functions
have further Value deserializers. Whether an existing record is recoverable
depends on its actual retained bytes; no restoration is inferred from this
source audit.

Candidate A introduces an explicit ordinary-object JSON policy, distinct from
the existing serde-Value compatibility policy, at the controlled arena parser.
The ordinary policy would disable private-sentinel dispatch at every nesting
level, preserve duplicate-last and exact numeric handling, and be used by SQL
JSONB input/canonicalization and its semantic operators. The serde-Value
policy would remain available for inputs whose existing contract explicitly
requires it, such as the JSON-array spelling of TEXT[] operands. Candidate B
changes existence only; it cannot recover keys already lost in JSONB casts,
COPY, parameter conversion or persistent representations. Candidate C leaves
the discrepancy explicitly open while tracing the full input/ownership and
storage handoff. Jev selected A (0.98) with a cross-entry and representation
audit before activation. It identified loss through a later serde Value
conversion (0.69), existing stored-data compatibility (0.85), parse/error
behavior (0.47), and resource ownership (0.66) as probe leads. Receipt:
`target/assembly-review/jsonb-sentinel-cross-entry-contract-{request,response}.json`.
An isolated existence or cast fix cannot establish the contract; the audit
must cover scalar and array wire/COPY, semantic keys/order, persistence and
round trips, functions and joins before routing production inputs. Already
lost stored data must not be assumed recoverable.

Any selected fix must preserve source-specific JSON vs JSONB diagnostics,
syntax/error priority, JSONB NUL rejection (22P05), numeric range (22003),
duplicate-last and JSONB key order, wire OIDs, UTF-8 and malformed-input
behavior. It must account for old/new owner overlap and cancellation without
new size/depth caps. Probes need both sentinel names as first and nonfirst
keys, nested objects/arrays, duplicate and escaped-equivalent keys, valid and
invalid sentinel-looking values, preexisting stored documents, SQL JSON/JSONB
casts, text and binary parameters/COPY, extraction/existence/other consuming
operators, and exact/below budget plus actual-work cancellation. Existing
Value-mode users and compatibility tests must remain stable unless the
selected product contract explicitly changes them. Midge remains the only
storage layer; do not infer an upstream engine feature or alter Midge.
