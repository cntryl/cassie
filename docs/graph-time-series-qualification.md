# Graph and time-series qualification

Issue [772](https://github.com/cntryl/cassie/issues/772) owns this finite review. The initial inspected source is `f510d76910a0c1bb192e4e6ab8d267c821f0215b`. This review does not promote support: graph traversal, time-series rollups and retention remain Experimental; fixed positive UTC minute/hour/day time-series indexes remain Stable. It does not select a persistent format, public API or shared statement snapshot.

## Finite invariant disposition

| Invariant | Retained execution evidence | Disposition |
|---|---|---|
| DOM001 typed node identity and simple paths | Existing case/type/cycle/self-loop owners rerun; independent authoritative edge DFS over64 tiny graph topologies with exact-case IDs, same IDs in other types, case aliases and cycles | Demonstrated for this finite fixture; no universal topology claim |
| DOM002 globally cheapest bounded simple paths | Existing shared-prefix/costlier-prefix/depth/weight tests;64 topologies ×4 depth bounds ×3 requested path counts =768 independent oracle comparisons, validating costs and every returned edge path | Demonstrated for this finite matrix; completed simple-path implementation retained |
| DOM003 atomic graph/query resources | Existing graph tiny-memory, deterministic entry cancellation, selected prefix/candidate controls, specialized graph/time-series path/fallback tests rerun; the prior cold read law is superseded below | Demonstrated existing controls; no latency or provider-wide promotion |
| DOM004 own writes, lifecycle and adjacency completeness | Existing own insert/update/delete/savepoint, cross-session and database isolation, rename/drop/restart owners rerun; live missing one type-specific member among valid neighbors returns a silently incomplete native result | Private authoritative verifier implemented; genuine missing-member RED and finite completeness/resource/benchmark GREEN recorded below. Issue772 remains open until final full and hosted acceptance |
| DOM005 UTC boundaries, partition identity and authoritative filters | Existing width/negative-epoch/parameter/subsecond/tab-partition/row-vs-index owners rerun; focused FLOAT signed-zero equality regression compares native retrieval with authoritative unindexed twin, including both stored spellings | Signed-zero repair widens zero partition lookup and retains exact residual filtering; no stored-format change |
| DOM006 incomplete membership and retention effects | Existing one missing member, dangling member, missing/corrupt count/manifest, stale generation, old/missing manifest restart, DML, retention/FK/projection/rollup failure/debt owners rerun | Demonstrated finite existing recovery and lifecycle controls; no new cloud/interruption guarantee |

The12 existing owner groups execute92 tests with0 failures. Their compiled source is `819c8932379a80d1168c89612a4b82cf6b420351`; source comparison with inspected main shows only unrelated storage fixture diagnostic idioms and deletion of four test-only cache debug prints. Graph/time-series runtime, tested fixtures and dependency manifests are byte identical. These reruns are qualified reused executables, not a claim of a new complete suite at main. Exact executable, input, result and source hashes are retained in the isolated worktree's ignored `target/qualification-772` records.

## Signed-zero partition repair

A FLOAT partition parameter `-0.0` is SQL-equal to `+0.0`. Previous partition admission serialized the parameter to `-0.0` and restricted bucket lookup to that spelling, silently omitting `0.0` members. The authoritative unindexed twin returns the zero rows; the indexed query returned no rows while publishing a native hit and no fallback. The actual red probe and [confirmed finding](https://github.com/cntryl/cassie/issues/772#issuecomment-6043514442) are retained.

`time_series_qualification::should_preserve_signed_zero_float_partition_rows` compares both zero parameters and a nonzero control, including directly stored positive and negative zero JSON payloads, exact rows, native counters, no fallback and released query memory. FLOAT zero no longer narrows membership to one spelling. The selected timestamp range remains bucket-native, all candidate partitions receive authoritative SQL residual filtering, and existing memory/cancellation controls apply. Nonzero FLOAT partitions retain narrowing. Existing persisted keys and generations are unchanged; composite partition predicates also widen when a zero component prevents exact narrowing. This conservative scan can read more candidate partitions for zero equality, preserving the result law.

## Open adjacency finding and decision boundary

The [live adjacency finding](https://github.com/cntryl/cassie/issues/772#issuecomment-6043589425) removes only one `OE` member while retaining valid neighboring members, all authoritative rows, redundant keys and unchanged source generation/manifest. With result caching disabled, the selected type-specific native scan silently omits that edge. Existing startup reconciliation compares exact source-derived key sets and rebuilds the missing entry. Live scan admission checks version/generation but has no per-prefix completeness proof.

The earlier signed-zero repair changed no graph runtime or persistent format. At that decision boundary, a whole authoritative graph scan would violate the then-current type-specific read bounds; a redundant-prefix comparison would not prove simultaneous missing members. Proposed new formats remain unselected. An authenticated successor chain was rejected during review because every predecessor digest changes after a suffix edit, causing degree-proportional write amplification. No approved exclusion or follow-up issue removes this acceptance criterion. Issue772 must remain open until the existing-law-preserving repair or an explicit support boundary is selected and verified.

On 2026-10-08 the user selected controlled authoritative verification for each
neighborhood, accepting linear reads and memory without a storage-format change.
The successor [performance contract](performance-contracts.md) replaces the cold
LIMIT 1 two-entry/four-read guarantee with counted O(N + K) verification followed
by the selected prefix read. Exact source-derived keys and all adjacency members
must agree within the same Data transaction before native publication. A
generation-only certificate is insufficient because raw sidecar deletion can
leave the source generation unchanged. Column-store sources require equivalent
same-transaction authority or the existing exact fallback. This decision is
implementation authorization, not execution evidence: DOM004 and issue772 remain
open until genuine regressions, controlled resource and lifecycle probes, finite
read/memory benchmark evidence and the complete validation gates pass.

## Authoritative verification repair evidence

The private verifier derives all four exact adjacency keys from authoritative
row records and compares every member in the same Data transaction before
native selection. Manifest count, version and source generation remain required.
It keeps no generation-only completeness memo. Missing, substituted, nonempty
or extra members select `incomplete-sidecar-membership`; column-store sources
use `unverified-column-sidecar` and the exact controlled row path. Failed native
verification reads are included in final fallback evidence.

The actual one-member deletion initially returned the wrong LIMIT 1 edge; the
repair returns the authoritative lowest-weight edge. Six focused controls pass:
that regression, warm removal of every redundant member, same-count substitution,
nonempty/extra keys, cancellation during source/adjacency verification and
late-member corruption beyond entry128. Existing 13 graph resource/lifecycle
owners pass with unchanged positive64KiB and negative budgets. The selected
one-edge startup scan counts seven entries; the 66-edge filtered read counts332.
The old two-entry/four-read assertions are preserved as superseded evidence.

Five private key controls cover preconstruction denial, escaped/numeric key
capacity, old/new slot admission at1-to2 and256-to512 growth, and geometric
capacity. The geometric fixture has genuine RED257 versus512 allocated slots,
then GREEN. A later ownership refinement retains slot and key owners together
through sorting/comparison and field destruction. Sorting/binary comparisons
add O(N log N + K log N) comparison work; geometric slot movement is amortized
linear. Midge-owned snapshot/block cursor and generic catalog allocations are
outside this query-owned retained-state claim.

The unchanged100k-node/99999-edge/depth4 graph representative correctly denies
its old64MiB profile. Two paged-cursor120-second deadline failures are preserved,
including the geometric-growth attempt. A single lazy Midge iterator per
verification namespace retains per-entry cancellation/deadline checks without
reopening each128-entry page. The normal test-profile smoke harness passes with
a dedicated `authoritative_graph_100k_128m` budget of128MiB: four candidates,
1,999,989 storage reads, peak accounted memory75,017,445 bytes, no cache hits or
fallbacks, and zero live query memory/workers. Other analytical profiles remain
64MiB and the runtime default remains10MiB. One smoke sample is explicitly
untrustworthy for latency/throughput qualification and establishes no SLA.

These finite results do not replace final immutable-head full/hosted validation.
Ignored evidence lives under `target/772-preparation/dom004-authoritative-verification/implementation/`.

## Validation boundary

The signed-zero repair requires build, complete locked tests, full workspace/all-target/all-feature pedantic Clippy, formatting and touched-test validators in repository order. Retained qualification probes do not replace those gates. Exact-head local/hosted results belong in the repair's PR engineering record; this document does not report pending checks as passed.
