# Selected column codec qualification

This record freezes a finite test and evidence slice of
[#768](https://github.com/cntryl/cassie/issues/768) at
`4b75413be837f2f9ecc41dba970bc76cf0c5034a`. Its native prerequisites,
[#758](https://github.com/cntryl/cassie/issues/758) and
[#763](https://github.com/cntryl/cassie/issues/763), were read back closed.
It records the existing CBC2 decoder contract from
[performance contracts](performance-contracts.md); it selects no new encoding,
format version, public API or runtime behavior.

The three new focused tests and three existing companion controls passed in
local execution on the dependency refresh
`55b52e26ac570554ba394ec06d45568853e2866d`. The only baseline changes from
the frozen contract revision were `Cargo.toml` and `Cargo.lock`; the production
decoder seams below are unchanged. The parent issue remains open for its
unqualified obligations.
This slice changes tests and qualification documentation only. No support or
native performance promotion follows from these fixtures.

## Finite obligation matrix

| Owner | Existing obligation | Disposition in this slice |
| --- | --- | --- |
| COL-01 | Logical Midge families, physical ColumnStore field keys and CBM2 sidecars are distinct structures. | Unchanged; database-family/layout qualification remains outside this slice. |
| COL-02 | Live-row marker and owned fields publish/delete atomically. | Unchanged; publication and interruption proof remains outside this slice. |
| COL-07 | Known framing/version, bounded lengths/counts and required checksums validate before accelerated publication. | Validity-padding decoder probe passed. Existing truncation controls were replayed; manifest/checksum/publication obligations remain separate. |
| COL-08 | Stable ordered nonoverlapping half-open ranges preserve each row across split/compaction. | Unchanged; range/lifecycle proof remains outside this slice. |
| COL-09 | One accelerated attempt belongs to one complete generation. | Unchanged; concurrent generation proof remains outside this slice. |
| COL-10 | One-row DML rewrites necessary ranges and preserves untouched identities. | Unchanged; write-amplification/lifecycle proof remains outside this slice. |
| COL-16 | Codec choice is deterministic with the declared complete-plain savings threshold and tie ordering. | Unchanged; this explicit decoder fixture does not qualify automatic encoder choice or savings. |
| COL-17 | Integer codecs preserve signed 64-bit values and validate packed boundaries. | Existing bitpack owners retained; no new integer-codec qualification claimed. |
| COL-18 | Accepted ALP values reconstruct exact float bits; unsafe candidates decline. | Existing ALP owners retained; no new float qualification claimed. |
| COL-19 | Constant chunks require identical scalar bytes, including signed-zero distinctions. | Existing constant owners retained; no new constant qualification claimed. |
| COL-20 | Selected dictionary/FSST decode validates complete framing, indices/symbols and UTF-8, including unselected positions. | Three dictionary/validity probes passed, with existing truncation and FSST controls replayed. Finite evidence only. |

## Production seam and existing controls

The existing hidden test helper
`decode_selected_column_chunk_for_test` in
`src/midge/adapter/column_batch_format_v2.rs` calls the production
`selected::decode`. No new helper API is needed.

`selected.rs::decode_dictionary_parts` validates every dictionary scalar with
`chunk.rs::scalar_from_bytes`, requires strictly sorted raw dictionary bytes,
and parses the complete frame-of-reference index stream. `selected::decode`
resolves and bounds-checks each non-NULL index before examining whether that
position is selected. `chunk.rs::validate_validity` rejects unused high bits
before either dictionary or FSST decoding.

The existing analytics test
`column_batch_format_v2::should_validate_selected_dictionary_payload_before_partial_decode`
checks a valid sparse selected result and every truncated-prefix boundary. It
does not isolate an out-of-range unselected index or malformed unselected
UTF-8 entry. Reuse this test rather than adding another truncation sweep.
The private FSST tests `should_reject_corrupt_unselected_fsst_value` and
`should_reject_invalid_utf8_in_unselected_fsst_value` already inject actual
unselected faults. Replay them; this slice adds no duplicate FSST fixtures.

## Explicit accepted-format fixture

A focused private builder in
`tests/analytics/selected_dictionary_validation.rs` constructs one 68-byte
CBC2 dictionary chunk. This is an accepted decoder-format fixture, not a claim
that automatic encoder selection would choose a dictionary for two short values.
The existing decoder accepts width two for these two indices; minimum packed
width and compression savings are encoder-choice obligations outside this probe.

All multibyte integers are little endian. The 34-byte header contains magic
`CBC2`, format version `2`, UTF-8 logical tag `4`, dictionary codec tag `3`,
flags `0`, value count `2`, NULL count `0`, validity length `1`, payload length
`33`, and decoded plain payload length `10`. The latter is two length-prefixed
one-byte UTF-8 values, not the total chunk size.

| Byte offset | Fixture field | Valid value |
| --- | --- | --- |
| 34 | Validity byte | `0x03`: both positions non-NULL, unused bits clear. |
| 35 | Dictionary count, u32 | `2`. |
| 39 | First scalar length, u32 | `1`. |
| 43 | First scalar byte | `a` (`0x61`). |
| 44 | Second scalar length, u32 | `1`. |
| 48 | Second scalar byte | `b` (`0x62`). |
| 49 | FOR block count, u32 | `1`. |
| 53 | FOR block length, u8 | `2`. |
| 54 | FOR base, u64 | `0`. |
| 62 | Bit width, u8 | `2`. |
| 63 | Packed byte length, u32 | `1`. |
| 67 | Packed indices, low bits first | `0x04`: indices `[0, 1]`. |

Every mutation starts from the same valid twin, changes exactly one byte, and
retains all lengths. The valid twin must identify codec `dictionary`, decode
fully to `["a", "b"]`, decode with selection `[true, false]` to `["a", NULL]`,
and decode with `[false, false]` to `[NULL, NULL]`. These controls distinguish an
actual selected dictionary path from accidental codec selection or malformed
outer framing. CBC2 helper calls do not pass through CBM2 manifest checksums.

## Three focused probes

Each test has a `should_` name and Arrange/Act/Assert. Assert the existing
`CassieError::Parse` category and exact contained diagnostic, avoiding a whole
Display-wrapper comparison. For every fault require full decoder rejection,
sparse selected decoder rejection, and all-false selected decoder rejection;
all-false selection must not skip validation of the complete chunk.

| Witness | Only changed byte | Required diagnostic |
| --- | --- | --- |
| `should_reject_an_unselected_dictionary_index_out_of_range` | Offset 67: `0x04` to `0x0c`; indices become `[0, 3]`, with only position 1 invalid. | `column-batch dictionary index out of range` |
| `should_reject_invalid_utf8_in_an_unselected_dictionary_entry` | Offset 48: `0x62` to `0xff`; scalar length stays one and raw order `a < 0xff` remains strictly sorted. | `invalid column-batch UTF-8` |
| `should_reject_unused_validity_bits_before_selected_dictionary_decode` | Offset 34: `0x03` to `0x83`; both live bits remain, with one unused high bit set. | `non-zero trailing validity bits` |

The guards already exist, so baseline PASS is expected and must be recorded
honestly. No fabricated runtime RED is required for a test-coverage addition.
If an actual existing-contract failure appears, record the failing input and
revision, create a separate categorized Bug linked as a native prerequisite,
and scope its repair independently before runtime edits.

## Evidence and completion limits

The initial qualification ran with `CARGO_BUILD_JOBS=2` and
`CARGO_INCREMENTAL=0`, the normal test profile and default test threading. These
are baseline PASS receipts for existing guards, without a runtime RED or repair.

| Actual command | Result |
| --- | --- |
| `cargo test --locked --test analytics selected_dictionary_validation:: -- --nocapture` | 3 passed, 0 failed. Every valid and corrupt twin exercised full, sparse and all-false decoding. |
| `cargo test --locked --test analytics column_batch_format_v2::should_validate_selected_dictionary_payload_before_partial_decode -- --nocapture` | 1 passed, 0 failed; existing valid sparse result and truncation sweep. |
| `cargo test --locked --lib unselected_fsst_value -- --nocapture` | 2 passed, 0 failed; existing corrupt-stream and invalid-UTF-8 unselected FSST controls. |

An earlier compile on the old dependency baseline was cancelled after the live
main refresh; it supplies no test outcome. Command, source-hash and output-hash
receipts are retained in the isolated worktree's ignored
`target/768-evidence/`. The PR validation record pins the committed head and
publication evidence.

Jev selected the explicit accepted-format fixture with probability 1.00. Its
fixture/oracle gap probability 0.56 required the three actual fault probes above;
the passing exact diagnostics and valid twins supply the local evidence. The
architecture review's all-false shortcut gap 0.37 required all-false positive
and corrupt twins, which passed in all three new tests. These judgments guide
probes and are not correctness or parent-issue completion proof.

The required ordered build, locked full tests, workspace/all-targets/all-features
pedantic clippy, format and touched-test validators, plus applicable
documentation/module policies, remain pending at this focused checkpoint.
Independent final review and exact-head hosted checks also remain publication
requirements; their actual status belongs in the PR validation record.

A passing finite decoder slice supplies evidence for the exercised COL-20 and
COL-07 boundaries. It does not qualify complete storage publication, corruption
fallback, restart recovery, capacity, native architecture or paired codec
performance. All eleven parent owners retain their actual wider acceptance
status. The PR should reference #768 as related work while those obligations
remain open.
