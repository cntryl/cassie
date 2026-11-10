# #809 private-sentinel round-trip gate

Status: the ordinary-object representation change is implemented and its local
bundle gates passed on 2026-11-10. This record qualifies current-format
behavior. Backwards compatibility and migration between unreleased Cassie
development builds are out of scope; current-format restart, recovery, and
database-image export/restore remain required.

## Contract

PostgreSQL treats `$serde_json::private::Number` and
`$serde_json::private::RawValue` as ordinary object keys at every nesting level.
Cassie's JSON carrier must preserve those keys through JSON and JSONB input,
operators, storage, results, and current-format lifecycle operations. JSON stays
OID 114; JSONB stays OID 3802 with canonical key ordering, duplicate-last keys,
and exact decimal behavior. SQL NULL remains distinct from document null.

## Implementation

`runtime::accounted::json_arena::value::parse_value` reserves query memory,
parses with `ObjectPolicy::Plain`, materializes the existing
`serde_json::Value` carrier, and checks interruption before returning. The
arena's Value policy remains available to callers that explicitly need its
previous behavior. JSONB canonicalization uses the same controlled ordinary
object parser and arena serializer.

The ordinary-object parser is wired through text and binary Bind, text and
binary COPY, REST document input, array values, casts, JSON functions and
operators, row encoding and decoding, and text/binary result codecs. Row reads
carry query controls into JSON decoding. The current-format database-image path
preserves the same values across export and restore.

## Executed evidence

- `src/runtime/accounted/json_arena/value/tests.rs` covers root and nested
  sentinel-looking keys, escaped duplicate keys, exact number lexemes,
  reservation lifetime, malformed input, resource limits, and cancellation.
- Text and binary Bind coverage is in
  `src/pgwire/connection/extended/text_parameters_tests.rs` and
  `src/pgwire/connection/extended/parameter_admission_tests.rs`. COPY routes are
  covered by `src/pgwire/connection/copy/input/binary_json_owner_tests.rs`,
  `src/pgwire/connection/copy/input/binary_jsonb_owner_tests.rs`, and the
  related text input tests.
- REST input and JSON operator behavior are covered by
  `tests/rest/json/semantics.rs` and
  `tests/pgwire_extended/types/json/operators/semantics/core_operators.rs`.
- Bind, storage, text/binary output, and current-format restart are covered by
  `tests/pgwire_extended/types/json/native_jsonb.rs`; database-image lifecycle
  coverage is in `tests/pgwire_core/databases/database_images.rs`.
- `cargo build --locked --bin cassie` passed.
- `cargo test --locked` passed across the workspace and every integration
  target. The final test-fixture-only adjustment was rerun with
  `cargo test --locked should_preserve_reserved_looking_keys_inside_nested_objects`.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` passed.
- `cargo fmt --all -- --check` and `git diff --check` passed.
- `cntryl-tools validate-tests` passed globally: 11,689/11,689 compliant.

Jev reviewed four concrete questions covering remaining input routes, current
storage round trips, memory/cancellation ownership, and JSONB semantics. Their
reported gap probabilities were 0.22, 0.16, 0.14, and 0.16 respectively, all
below the repository's 0.30 focused-probe threshold. Source and executable
regressions remain the qualification evidence; Jev's judgments are leads only.
The redacted request and response are retained under ignored
`target/jev-809-canceled-audit.json`.

## Scope boundary

No old Cassie build, storage format, image, binary parameter, or Rust API is a
supported input. Independent JSON feature limits remain listed in
[Feature Support](feature-support.md); this record does not promote those
features beyond their documented status.
