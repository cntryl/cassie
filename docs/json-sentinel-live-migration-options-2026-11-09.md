# Historical #809 storage-migration analysis — superseded

Status: **superseded on 2026-11-10**. The user clarified that Cassie is
unreleased and backwards compatibility or migration between development builds
is not required. The old-reader, mixed-version, rollback and migration options
in this analysis are no longer active gates.

The source evidence about sentinel-key ambiguity remains relevant: ordinary
`$serde_json::private::Number` and `$serde_json::private::RawValue` keys can be
misread by serde Value-mode parsing, and already transformed bytes cannot reveal
the original object. Those bytes belong to unsupported pre-release formats and
need no repair or migration path. The selected current-format parser direction
and remaining acceptance work are recorded in
[JSON carrier decision](json-ordinary-representation-options-2026-11-09.md) and
the [#809 round-trip gate](json-sentinel-roundtrip-gate-2026-11-09.md).

Cassie continues to use published `cntryl-midge` APIs as its only storage layer.
The current build must preserve current-format restart and recovery behavior.
