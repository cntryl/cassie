# #809 private-sentinel round-trip gate

Status: the guarantee that ordinary sentinel-key objects round-trip unchanged
across all JSON/JSONB entries, storage, operators and public results is **punted
pending the migration and reader-fencing dependency below**; no runtime
migration or reserved-key rejection is selected. The JSON operator issue
remains open and no part of its remaining work is declared complete. Separately,
an already transformed legacy value cannot be restored to its original object
from indistinguishable row bytes. Such repair requires an authoritative
external source and an explicit replay or rewrite; guessing from a scalar is
not permitted. Fresh ordinary objects and legacy bytes which still contain them
remain #809 obligations, not completed or permanently excluded cases.

## Evidence and current boundaries

PostgreSQL 18.6 treats `$serde_json::private::Number` and
`$serde_json::private::RawValue` as ordinary keys. With the locked serde_json
features, Value-mode parsing may instead interpret a first decoded key as its
private number or raw-value sentinel, including in nested objects. This occurs
before a later operator can recover the original object. Escaped spellings,
duplicate keys and key ordering matter; a nonfirst key at one boundary may
become first after a map is serialized. Plain JSON raw operators and Value-mode
consumers do not all take the same route.

The current text Bind, binary JSONB Bind and JSONB canonicalization routes
construct or reparse serde Values. Binary COPY's controlled arena intentionally
uses Value-mode sentinel semantics. `TYPE_JSON` in CRB1 serializes a serde
Value and `decode_row` reparses it, as do projected row decoders. JsonbKey and
other semantic/function consumers reparse text or Values. An embedded caller can
also construct a serde map directly. `Value::Json(serde_json::Value)` and
`DocumentRef.payload` are public; `QueryResult::rows()` returns an infallible
borrowed slice of those Values. A private raw cell or pgwire-only formatter
alone cannot provide an equivalent embedded result or public adapter payload.

The retained, **unexecuted** isolated witnesses at
`cassie-json-sentinel-roundtrip-20261109` include both Number and nested
RawValue row-blob cases, SQL/Bind/COPY/restart cases, and a control showing
that a sentinel object already transformed to numeric 7 and a genuine numeric
7 can encode to identical row bytes. They are not proof of runtime behavior or
permission to overwrite other work. Their restart fixture currently calls a
helper that mutates process-wide storage environment; before use, replace that
fixture route with explicit `Cassie::new_local_with_data_dir_and_config`, an
isolated owned directory and an actual close/reopen. Do not promote or execute
that WIP until its base and all paths are reconciled.

## Choices and current decision

* **Versioned owned representation.** Continue designing a Cassie-owned JSON
  representation and an explicit compatibility path for public Value,
  DocumentRef, serde, Clone and result APIs. Preserve original valid old bytes,
  distinguish SQL NULL from JSON null, and define JSON spelling versus JSONB
  canonicalization, duplicate-last, exact numbers and diagnostics. A new
  durable tag/version and old-reader fence may be required. This is a design
  direction, not a proven source-compatible or admission-safe implementation.
* **Reject reserved-looking keys.** A fail-closed guard would reject valid
  PostgreSQL JSON and previously accepted Cassie JSON; no guard at a single
  input catches embedded serde maps, all Bind/COPY forms, nested/escaped keys,
  existing stored bytes and later reparses. It cannot identify already-lost
  input. It is not selected as a compatibility-preserving fix.
* **Leave the guarantee open.** Preserve current behavior and explicitly report
  the known semantic divergence until an end-to-end implementation and its
  compatibility/fencing proof exist. This is the selected present disposition,
  not a claim that the rest of #809 is complete.

Jev's focused current gate gave leave-open 0.52, conditional design 0.48 and
reserved-key rejection 0; the follow-up rated neither production route
source-proven 1.00 and the need to repair/document witnesses 1.00. The 0.92
missed-gap lead requires the focused entry, representation, owner and mixed-reader
probes below. Receipts are in ignored `target/assembly-review/` under
`809-json-sentinel-current-gate` and `809-json-sentinel-deliverability`.
Jev is a decision aid, not executable proof.

## Published Midge boundary and minimum-reader proof

The published `cntryl-midge` 0.5.2 public API exposes Engine, single-family
transactions, scans, shutdown and primary-lease health. Its Engine startup
acquires a primary lease: local storage uses a filesystem lease, cloud uses a
provider-backed lease, and a competing owner can receive `LeaseHeld`. Its API
documents cloud lease-loss fencing, including existing cloud transaction reads,
and shutdown retains writer fencing while workers finish. These are real
engine capabilities; they are not an application JSON-layout epoch barrier or
proof that every already-open Cassie consumer revalidates a marker.

Cassie's current `ensure_families_ready` caches `StorageLayout` after admission.
The inspected merged 0.1 baseline `7b9d29f4` checks the marker during bootstrap
and rejects an unknown value; CRB1 RowDirectory also rejects an unknown format
version. Neither observation alone proves that every older release, already
initialized handle or supported storage topology is fenced during an online
upgrade, nor that a schema-family marker and data-family write are atomic.
The public Midge API inventory did not identify an application layout-change
barrier that invalidates every existing Cassie reader. Do not invent one or
edit Midge source. A stopped-instance/lease-exclusive upgrade is a candidate,
not yet a qualified guarantee: prove quiescence, marker durability, old and new
binary startup, retained handles, cloud/local behavior, crash/restart and
rollback/downgrade before writing a new representation.
After the actual Midge lease evidence was supplied, Jev selected offline
stop/reopen as a candidate only (1.00; proven and unavailable both 0), with a
0.95 proof-gap lead. Its receipt is
`target/assembly-review/809-offline-midge-fence-{request,response}.json`.
The lead requires synchronized already-open-handle and old-release probes,
local/cloud quiescence and lease-loss, marker/data durability and crash, and
rollback/downgrade evidence; it does not establish a migration fence.

## Smallest coherent production bundle and dependency

No source-proven runtime bundle currently crosses all required boundaries.
The next implementable contract work is to repair the isolated restart fixture,
then establish actual marker/lease behavior against each supported old binary
and storage mode using owned, synchronized processes. In parallel, specify and
source-prove an owner-coupled document, public compatibility/fallible
materialization and exact serialization semantics. The smallest candidate live
route is a fresh JSONB text Bind through DML and the published Midge byte API,
full/projected row decode, semantic keys, expression/result ownership and
text/binary pgwire, with an explicit, proven embedded-public-result policy.
Other entry routes must retain their documented behavior until migrated; this
single route would not complete #809. Subsequent increments must cover SQL
casts, binary Bind, text/binary COPY and the remaining consumers. Every route
must cover NULL, wide numbers, duplicate/escaped and nested keys, NUL/error
order, cancellation, exact/below budgets and failure drops. Do not enable a
private fast path that silently falls back to sentinel-aware serde before its
handoff is complete.

Dependency to resolve before activation: a supported Cassie-side migration and
minimum-reader/active-handle policy using the published Midge API, with the
public representation and ownership conversion source-proved. If those
capabilities cannot be established, keep the fresh round-trip guarantee open and
retain the specific blocker; do not substitute reserved-key rejection or claim
recovery of already lost data. Builds, tests and full qualification remain
explicitly deferred to the consolidated gate.
