# Controlled finite JSON path functions

For `json_extract_path`, `json_extract_path_text`, `jsonb_extract_path`, and
`jsonb_extract_path_text`, retain the existing initial control check,
arity/SQL-NULL checks, JSONB input-size validation, workspace admission and
`json_text` conversion. With both controls and an expression owner, reuse the
checked raw scanner and selection/formatting used by the operators, borrowing
the variadic segment Values. Parse the whole source before visiting segments;
non-String segments still cause NULL only when reached. The output owner is
adopted while the existing workspace remains alive and that workspace then
follows the existing value-first transfer. Ownerless and no-controls calls
continue through the legacy function path.

Jev selected the scope (0.92); probes must cover malformed/NULL/lazy segment
and JSONB error order (0.82), workspace/output lease overlap and partial
failure (0.55), and fallback/cancellation priority (0.74). Receipt:
`target/assembly-review/json-path-function-raw-consumer-contract-{request,response}.json`.
Additional admission and interruption can precede the old result. Input/argument
cloning, existing JSONB preflight, allocator excess, raw diagnostics, recursive
teardown, sentinel/storage migration and no-owner paths remain unqualified.
All probes remain unexecuted until the consolidated gate.
