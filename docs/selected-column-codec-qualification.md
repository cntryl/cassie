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
| COL-16 | Codec choice is deterministic with the declared complete-plain savings threshold and tie ordering. | The original decoder checkpoint did not qualify encoder choice. The later finite selection-policy qualification below passed its three focused encoder tests; companion controls and final validation remain pending. |
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


## Finite COL-16 selection-policy qualification on 2026-10-10

This later finite qualification belongs to
[#903](https://github.com/cntryl/cassie/issues/903), a finite child of #768,
at source baseline `0c754c12edf82d1c68d7519d010a5af9ad46fbdc`.
The earlier dictionary checkpoint and its execution record above remain
historical evidence. The original decoder fixture did not test automatic
selection. Native prerequisites #758, #763 and #880 were read back closed
before this candidate was prepared. Parent #768 remains open.

The existing [performance contract](performance-contracts.md) requires a codec
to save at least `max(32 bytes, 5% of the complete plain representation)`.
`chunk.rs::encode_with_policy` uses the ceiling of complete Plain bytes divided
by twenty and retains the earlier candidate on equal complete size. Its existing
candidate order is Constant, RLE, Dictionary, Frame-of-reference, FSST and ALP,
subject to logical-type eligibility. The ordinary storage caller is
`column_batches/storage_v2.rs::encode_segment` through `encode_column_chunk`.
This slice selects no new encoding, selector, format version or public API.

CBC2 complete sizes include its 34-byte header, validity bitmap and payload.
The following literal sizes are independently derived from this existing format;
they were asserted and matched by the focused execution recorded below.

| Finite law and input | Complete Plain bytes | Actual forced candidate expected bytes | Automatic choice expected |
| --- | --- | --- | --- |
| Two identical ASCII strings of length 23 / 24 / 25 | 89 / 91 / 93 | Constant 58 / 59 / 60, saving 31 / 32 / 33 | Plain 89 / Constant 59 / Constant 60 |
| 5,744 / 5,745 Int64 positions, first six 42 and all remaining NULL | 800 / 801 | Constant 760 / 761, saving 40 / 40 | Constant 760 at required 40 / Plain 801 at required 41 |
| Eighteen Int64 values, nine 0 then nine 255 | 181 | RLE 73 and Frame-of-reference 73 | RLE 73, the earlier equal-size candidate |

The percentage pair changes one validity byte while retaining the same 48-byte
Plain scalar payload and 8-byte Constant payload. It distinguishes the complete
Plain percentage base from a payload-only base. The equality cases pin admission
at exactly 32 bytes and exactly five percent. The compressed tie pins the existing
strict-smaller replacement rule without duplicating selector arithmetic in tests.

Three private `should_` tests in `chunk/selection_policy_tests.rs` invoke the
existing forced encoder for eligible Constant, RLE and Frame-of-reference
candidates before automatic selection. Every input also has a real forced Plain
control. Each forced and selected chunk must pin codec, logical type, total and
NULL counts, complete and decoded lengths, and exact round-trip values/NULL
positions. Two ordinary invocations must return byte-identical chunks; accepted
selection must equal the forced winner, and rejection must equal forced Plain.
No storage engine, SQL server, new test hook or independent codec is introduced.

### Actual focused execution and acceptance boundary

On the unchanged production baseline
`0c754c12edf82d1c68d7519d010a5af9ad46fbdc`, the three focused encoder
tests passed: **3 passed, 0 failed, 0 ignored**. All nineteen forced and
selected chunk records matched the literal sizes, codec identities and complete
round-trip expectations above. This is a baseline qualification PASS for existing
behavior; no runtime RED, repair, native result or support promotion is claimed.

The actual command was `cargo test --locked --lib selection_policy_tests:: --
--nocapture`, with ordinary profiles/default test threads, `CARGO_BUILD_JOBS=2`
and `CARGO_INCREMENTAL=0`. The isolated worktree retains its source, runner,
metadata, fully rehashed dependency-only seed and actual output under
`target/903-evidence/actual-selected-20261010/`. The command log SHA-256 is
`dc71a6e25aad49539b530de5265a4763bc55b1589088637230d48e825b897d58`;
the actual executed and preserved test binary SHA-256 is
`e78b517efb637f3c2c1e5f27b60413786a73e4f5183721a0e432399809ac01e4`.
Independent review verified the exact three tests and nineteen records.

Jev's fixture-isolation gap 0.63 and candidate-boundary gap 0.37 required these
actual forced-candidate and automatic-selection witnesses. Their actual passing
outputs supply that focused evidence. The three existing SQL companion controls,
full ordered validation and final-head publication gates remain pending at this
checkpoint. Run the following companion commands and replay the focused tests
as part of final validation:

```sh
cargo test --locked --lib selection_policy_tests:: -- --nocapture
cargo test --locked --test analytics column_batch_format_v2::should_select_each_typed_codec_deterministically -- --nocapture
cargo test --locked --test analytics column_batch_format_v2::should_fall_back_to_plain_when_savings_do_not_clear_the_threshold -- --nocapture
cargo test --locked --test analytics column_batch_format_v2::should_reduce_representative_compressible_bytes_by_at_least_twenty_five_percent -- --nocapture
```

Then complete the required ordered plain locked build, complete locked tests,
full workspace/all-target/all-feature pedantic Clippy, format and touched-test
validators, applicable repository policies, independent review and exact-head
hosted checks. Execution receipts must pin source, command, log and binary hashes.
The companion, full-validation and publication gates remain pending at this focused checkpoint.

This child can qualify only the finite COL-16 floor, complete-percentage and
compressed-tie boundaries above. Wider codec/numeric/selected-reader behavior,
column-family/physical-layout mapping, marker/field atomicity, framing/fallback,
publication, lifecycle, generation and range obligations, and retained native
paired-codec objectives keep their individual evidence requirements. #768 remains
open; the earlier #890 decoder evidence is neither duplicated nor expanded into
a complete parent claim.
