# Runtime and storage qualification — 2026-11-16

This finite diagnostic slice advances [#1191](https://github.com/cntryl/cassie/issues/1191)
and records the unchanged [#1188](https://github.com/cntryl/cassie/issues/1188)
dependency boundary. Both issues remain open. It adds a reproducible phase
profiler and enables both workspace packages to inherit the existing Clippy
lint policy. It changes no database or wire behavior.

## Fresh fixture profile

Command, from the repository root:

```sh
cargo bench --locked -p cassie-integration-tests --bench tier4_pgwire_fixture_profile --profile test
```

The integration package owns this explicit benchmark because it consumes the
same compiled fixture library as the four wire correctness targets. It runs
eight separate memory databases, owned bound listeners, real root password
startup, eight `SELECT 1` cycles per connection, and socket/listener stop. Each
query checks its returned row. `CASSIE_FIXTURE_PROFILE_SAMPLES` changes the
sample count. Timings are diagnostics, never correctness thresholds.

Two consecutive local macOS arm64 runs used the same source based on main
`9d25356a`, with only this profiler and manifest lint inheritance added. The
first built fresh private workspace/dependency artifacts in 2m25s. The second
reused those artifacts; compilation is excluded from the CSV stage timers.
No other owned Cargo process ran concurrently. Runtime background work and host
scheduling were not isolated, and the samples show substantial variation.

| Stage, median microseconds across eight fresh fixtures | Initial run | Warm-artifact run |
| --- | ---: | ---: |
| Memory fixture construction and Cassie startup | 95,831.5 | 26,387 |
| Bound listener and TCP connect | 170.5 | 186 |
| Password authentication through ReadyForQuery | 39,461.5 | 43,375 |
| Eight query/result cycles | 51,079.5 | 20,486.5 |
| Socket-half drop and accept-loop stop | 34 | 31 |

Accepted connection-task cleanup, runtime construction/destruction and engine
background-thread cleanup are outside the phase timers. In particular, listener
stop aborts/awaits only the accept loop, not each accepted connection. These
measurements do not establish complete fixture teardown cost. Timer and
assertion overhead is included in the measured phases.

These are separate medians, not an additive end-to-end median or a speedup
comparison. The harness is a small shared-fixture witness; it does not execute
or independently profile all assertions in the four named targets. The full
target timings and current hosted workflow are in
[Integration test compilation](compilation-organization.md#current-workflow-and-profiling).
Raw local receipts are under ignored
`target/assembly-line/runtime-qualification/profile-{baseline,warm}.log`.

### Selected bounded fixture improvement

The ten startup helpers in `tests/pgwire_extended/{subqueries,types/json,
simple_query,protocol,prepared_statements,portals}` that bind an ephemeral
listener, drop it, ask `server::run` to bind again, and sleep 50 ms will instead
transfer that same bound listener to `server::run_with_listener`. The bound
socket already admits connections; the operating system queues them until the
server task accepts. This removes the port-rebinding race and the fixed startup
delay while retaining a fresh database, real authentication, framing assertions,
runtime settings, cancellation and resource limits. The existing behavioral
tests remain the regressions; intentional post-cancellation waits stay intact.

This scope was selected with source-backed Jev review (probability 0.95).
Removing a configured sleep is a source fact, not measured end-to-end headroom.
The assembler must record the named target timings and complete ordered gates
on the integrated source before reporting an observed runtime improvement.

Source review confirms `write_frames` already coalesces ordinary cycles and
`authenticate_principal_inner` prepares real password hashes and verifies the
supplied credential. The measured authentication stage is material, but these
samples do not isolate hashing, query execution, background storage or
transport delay. Sharing databases, skipping authentication or changing
transport lacks isolation/speedup evidence. The selected listener transfer
does not attribute these measured stages to any particular implementation.

Jev selected the finite profiler/documentation scope with probability 1.0;
the probability that these measurements establish a safe material improvement
from sharing databases or changing transport was 0.03. That decision preserves
the issue's profiling-first contract; it does not prove correctness or claim
runtime improvement. Request/response receipts are under the same ignored
directory as `jev-scope-{request,response}.json`.

## Published storage dependency

`Cargo.lock` still pins published `cntryl-midge` 0.5.2. A live sparse-registry
readback on this date returned 0.5.2 as its last entry, with `yanked=false`.
[Upstream #1045](https://github.com/cntryl/midge/issues/1045) remains open,
without a recorded fix or isolated reproduction in its issue discussion.

The already merged Cassie repair remains present: `create_physical_family_with`
and physical drop participate in schema publication in
`src/midge/adapter/databases.rs`; `ensure_families_ready` serializes bootstrap
and `bootstrap_families` does not replay lifecycle recovery after initialization
in `src/midge/adapter/core.rs`. Existing controlled publication/bootstrap tests
and the disk-backed concurrent-create/retry/restart assertion remain intact.

The remaining #1188 criterion is broader native/background durability and
concurrency qualification. Native writable handles can commit beyond an
adapter issuance window; family flush and cloud-frontier waits do not provide
a global/background WAL-quiescence contract. A caller idle check does not
fence the internal drain-to-sync overlap described upstream. No new storage
capability or string-based retry of native Internal errors is introduced.
The next action remains an upstream confirmation/refutation or published
contract/fix, then a focused downstream reproduction and recovery qualification.

## Evidence status

The two profiler runs passed all 128 query-result checks (16 fixtures, eight
queries each). They are diagnostic execution evidence, separate from the
complete ordered local gate and final-head hosted checks recorded in the PR.
Clean execution or restart is not crash/power-loss or production qualification.
