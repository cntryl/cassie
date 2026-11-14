# Plain Boolean typed handoff qualification

This finite [#1265](https://github.com/cntryl/cassie/issues/1265) optimization follows the [primitive measurement](typed-primitive-profile.md) merged through #1266 at baseline `f857c8a35b85d8c895ca1fb2324f4c96332362ac`. It targets the observed 100,000 JSON-backed Boolean lanes and temporary Value slots in each covering-index query. It does not introduce a storage format, SQL capability, public API, kernel, runtime default or native-engine change.

## Selected boundary

Existing CBC1 Plain Boolean payloads store compact non-NULL bits mapped through logical validity. The native path decodes to an admitted nullable primitive Boolean owner, retaining raw bytes, metadata and the reservation through columns, batches, selections, slices, gathers and aliases. Final drop releases backing. Numeric native handoff is unchanged. Constant, RLE and Dictionary Boolean codecs retain their fully validated JSON/Value boundary.

Reuse current framing, checksums, manifest/type/codec/count/null/length checks and the unchanged conservative field reservation: encoded length + 64 × bounded declared decoded length + row count × max(sizeof(JSON Value), 64) + field/header overhead. Expected row domain is validated before native allocation. Header decoded length retains its current bound/manifest-equality meaning; it is not replaced by a recomputed JSON length. Unused validity bits remain rejected; unused high payload bits remain accepted. No writer policy or persisted bytes change. Missing/corrupt/stale required derived input preserves whole-attempt authoritative fallback, while resource/cancellation failures keep their existing terminal boundaries. Captured/staged source visibility and existing Boolean three-valued/numeric aggregate semantics remain authoritative.

## Comparison contract

All six existing `tier3_system_typed_primitive` workloads retain exactly the same 100k nullable BIGINT/BOOLEAN relation, SQL, independent exact SUM/COUNT/type oracle, one worker, cache-off execution, query 2 GiB / native total 2 GiB / memtable 512 MiB profiles and standard harness. The corrected before owner is `e10068c9cf9e9e5533dbcbba8a3c1c17169bd906`; original indexed CPU evidence remains explicitly `c3fd17f8e2d9c6893766604e17985bcbef3e1b66` and its executable SHA. Those immutable identities are not relabeled as new source or runtime git metadata. The merged baseline benchmark and catalog source are byte-identical to the preserved e100 owner.

The selected after evidence requires 100,000 encoded numeric lanes plus 100,000 `encoded_boolean_lanes`, with zero Boolean JSON/Value transport in indexed Plain cases. Other sources and non-Plain fallback retain truthful classifications. Inline Value slot flow and admitted query memory are not actual allocation traffic or RSS. The same indexed TRUE query gets a separate actual-PID CPU window after setup with private collection off; profiler-overhead timing is not canonical. Before/after rates, noise or regression must be reported without an invented speedup threshold. Allocation counts/bytes remain unavailable without an actual allocation profiler.

## Exact qualification and compatibility witnesses

The implementation is fully qualified locally at `889d493fa5e939852757ed8c942260afa8a9d75b`, tree `04d467ce30a6b8d5000f62e3e58231fa70fb51cb`. All benchmark main/helper/catalog sources are byte-identical to merged baseline `f857c8a35`; `chunk.rs` remains unchanged at 999 lines. Numeric behavior, non-Plain codec fallback, persistent bytes/tags, public APIs, defaults and CI/release/version configuration are unchanged. The additional private owner header is included through the existing `sizeof` term; no reservation-estimate optimization was made.

| Acceptance boundary | Executed witness |
| --- | --- |
| Compact NULL/non-NULL mapping and bit boundaries | `column_batch_format_v1/boolean_tests.rs` differentially checks domains 0,1,7,8,9,63,64,65,1023,1024,1025, mixed/all-NULL values, accepted payload padding, rejected validity padding and malformed framing |
| Expected domain before native construction; unchanged validation | Same decoder probes plus `column_batches/typed_source/boolean_tests.rs` check metadata disagreement, checksum/truncation/format failures and actual source preparation |
| Constant/RLE/Dictionary compatibility | `should_keep_nonplain_boolean_chunks_on_the_validated_json_path` retains actual generic decoding and released admission for all three codecs |
| Complete-source failure/fallback | Required Boolean checksum and checksum-consistent malformed-chunk probes discard a complete candidate, including a later failed segment after an earlier segment loaded; public SQL matches authoritative fallback |
| Backing lifetime, views and controls | `typed_batch/column/boolean_tests.rs` checks repeated gather/slice/clone aliases, declared-type rejection, denied admission and canceled construction; final owner releases query reservation |
| Scan advancement, native cells, SQL and staged visibility | `scan/typed/profile_tests.rs` checks repeated selected aliases over parent/source advancement, exact >2^53 SUM/count/OIDs and no JSON/Value transport; staged INSERT/UPDATE/DELETE use controlled documents, rollback restores the committed native source |

Seventeen new tiny probes and the updated existing native-lane trace probe all passed. Focused regressions were authored before implementation; additional adversarial probes were added during the same increment. All execution was deferred; no baseline RED was observed. Input and complete integrated reviews identify exact base/head/tree and source-backed Jev dispositions. A model judgment does not replace executable qualification.

All times below are UTC on 2026-11-14. The integration target was private, jobs=2/test threads=4.

| Command/check | Actual result |
| --- | --- |
| `cargo build --locked --bin cassie` | Passed 18:51:26 |
| `cargo test --locked` | Passed 19:08:16: 13,529 passed, zero failed, nine external-fixture tests ignored; 44 result blocks |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | Passed 19:09:02 |
| `cargo fmt --all -- --check` | Passed 19:09:07 |
| `cntryl-tools validate-tests -f <path>` | All five touched source-test files passed 19:09:07 |
| `tools/test_inventory.py --check` | Generated inventories current; nonzero for unchanged baseline five unregistered pgcrypto tests and one oversized parser file; no new unresolved modules, duplicate identities or destination conflicts |
| Six canonical after cases and untimed diagnostics | Passed 19:10:05–19:13:16, including optimized compilation |
| Separate indexed TRUE CPU window | Completed with sampler/process exit zero at the same source |
| Final-head hosted checks/review/squash/issue readback | Actual terminal readback recorded in [#1265](https://github.com/cntryl/cassie/issues/1265) after receipt documentation is committed |

## Actual before/after comparison

Before: the immutable preserved `e10068c9` executable SHA-256 `5e13bd2279dd44575faaa706a5ece7d57c07650715c06acbaf473151489c7bda` ran directly, without Cargo, from restored read-only detached `e10068c9` at its original manifest/cwd path. The runtime artifact was verified to name that actual source, not the new implementation. The fresh before run completed 18:38:42–18:39:23; original historical parent artifacts remain separate and unchanged. After: normal `cargo bench --locked --bench tier3_system_typed_primitive -- --output-dir <receipt-directory>/benchmark-artifacts` at `889d493f`, with an explicit diagnostic evidence path. Both used the default unfiltered complete owner, six specs/summaries, 36 samples (one warmup/five measured, 500 ms target) and worker metadata one. All after cases have acceptable quality and zero failed operations. The fresh before NOT FALSE cases were noisy and were not rerun to select a preferred sample.

Hardware/toolchain: Apple M5, ten cores, 24 GiB RAM, macOS 27.0.1 arm64, Rust 1.100.0, optimized release build, cntryl-stress 0.5.1 and cntryl-midge 0.5.2. One paired database contains all three 100k source variants. Query 2 GiB / native total 2 GiB / memtable 512 MiB, actual native transaction pool 214,748,364 bytes, one worker, cache off and no query deadline were identical. After setup took 6.843 seconds. Native diagnostic snapshots remained Healthy without write stalls. Timed private collection stayed disabled; the timed closure includes SQL and tiny exact result/type checks, excluding diagnostics and profiler overhead.

| Workload | Fresh before queries/s | Before quality | After queries/s | Raw after/before |
| --- | ---: | --- | ---: | ---: |
| `row_true` | 6.1563 | acceptable | 5.8975 | 0.958 |
| `column_true` | 3.9222 | acceptable | 4.1069 | 1.047 |
| `indexed_true` | 83.3757 | acceptable | 116.8782 | 1.402 |
| `row_not_false` | 5.3183 | noisy | 6.1246 | 1.152 |
| `column_not_false` | 3.5031 | noisy | 4.1552 | 1.186 |
| `indexed_not_false` | 67.4065 | noisy | 110.5380 | 1.640 |

The indexed TRUE primary comparison is an observed local 1.402× mean-rate ratio: before 83.3757 queries/s (95% interval 82.9152–83.8362), after 116.8782 (116.2640–117.4923), both acceptable. This is one platform and one before/after sequence, not a universal speedup or a preselected threshold. The non-indexed row TRUE control regressed 4.2% in this sequence (6.1563 to 5.8975); column TRUE increased 4.7%. Within-run intervals do not establish the cause of control changes or account for every between-process effect. No claim that every workload improved is made. Companion NOT FALSE ratios are raw observations only: their noisy before samples do not support a precise benefit estimate. No rerun hunting, default/profile/SQL change or timing assertion was used to hide these outcomes.

## Transport, resources and semantics

All six actual diagnostic queries matched the independent exact sums/counts and descriptors: SUM NUMERIC OID 1700, COUNT OID 20. TRUE result `(27021597771773687,15121,15121)` and NOT FALSE `(54043195543578022,30288,30288)` remain identical across sources. Each observed 98 completed batches over 100k logical input rows and one typed aggregate completion, with selected rows 15,121/30,288. No aggregate-acceleration dispatch or result-cache hit replaced the typed path.

| Indexed Plain Boolean evidence, each predicate | Before | After |
| --- | ---: | ---: |
| Native encoded numeric lanes | 100,000 | 100,000 |
| Native encoded Boolean lanes | No native owner; JSON-backed route observed | 100,000 |
| Encoded JSON-backed Boolean lanes | 100,000 | 0 |
| Temporary Value carrier slots | 100,000 | 0 |
| Inline Value carrier byte flow | 4,800,000 | 0 |
| Generic materialized-value counter | 100,000 | 0 |
| Segments/chunks | 98 / 294 | 98 / 294 |
| Row-blob fetches / fallback scans | 0 / 0 | 0 / 0 |

These are logical lanes and cumulative inline slot flow, not allocator counts, traffic, live peak bytes or RSS. Native decoding still constructs an admitted primitive nullable Boolean vector; it is not a compressed zero-copy path. Non-Plain Boolean JSON/Value fallback is separately validated by tiny codec/source probes. Ordinary heap/authoritative-column controls still report 200k document field/Value lanes (9.6 MB inline flow); authoritative columns retain 400k logical validation entries/marker attempts. No integrity checks were bypassed.

The indexed operator-boundary admitted peak changed from 95,052,384 to 94,957,349 bytes for TRUE (NOT FALSE 95,052,389 to 94,957,354). This small tracker change is not the 4.8 MB cumulative Value-flow figure or an allocation saving. The conservative reservation formula remains unchanged; source owners are intentionally live at operator completion, and every diagnostic SQL return ended with zero current accounted query memory. Private peaks are observed at operator completion, not a claim about later final-result peaks; public runtime high-water remains cumulative.

## Separate indexed CPU evidence and limits

After source `889d493f` executable SHA-256 `33e77969fc80e2ef04a14cc542276f5492af4ab34c42a0f2bda5a9b722027e3a`, actual PID75543, announced setup-complete ready at 19:14:02. `/usr/bin/sample 75543 5 1 -file <output>` ran 19:14:02–19:14:07 inside the separate 30.0046-second checked-query window; sampler and process exited zero. The window completed 3,242 checked queries; that profiler-overhead loop count is not a canonical benchmark rate. Private collection was off. Original before CPU remains source `c3fd17f8`/SHA `6baccb3e308987d14b5a5a48767da80c354a8d51ddcfcfe27897377f6360f18b`, PID40519, with its actual five-second sample and 30-second window; no artifact was relabeled as a different binary/source.

The after main-thread tree has 3,743 root samples; predicate branches 1,422, row-ID load 735 with decode 664 nested, and Boolean load 88 with native decode 84 nested are observable. Rust v0 names are partly identifiable mangled symbols and addresses, not complete line attribution. Nested branches and sleeping engine threads must not be summed or converted into an exclusive CPU decomposition. Source and positive lane/owner evidence establish removal of the JSON/Value transport route. Absence of a JSON symbol in a partial/inlined sample does not prove zero CPU cost. Remaining predicate/row-ID/validation work is not declared a new global bottleneck.

Actual allocation counts/bytes remain unavailable. The successful CPU sampler is not an allocation profiler; the parent tool readback found `xcrun --find xctrace` unavailable under CommandLineTools despite the wrapper. No tool installation, RSS/minimum-memory claim, all-codec/general reader claim, native-engine change or DuckDB comparison is made. Source-backed Jev selected this bounded interpretation, with noisy-companion precision and CPU-absence leads verified and reflected in these limits.

Exact input, complete integrated and final receipt reviews plus before/after raw JSON/CSV/Markdown, diagnostic traces, sampler commands/windows, source/binary identities, all gate logs and baseline policy outcomes are preserved under ignored `target/assembly-line/boolean-handoff/` before cleanup. The [owning issue](https://github.com/cntryl/cassie/issues/1265) records actual final-head hosted checks, formal review-status readback, squash tree and issue closure. Package publication and Linux disk/production qualification remain separate.
