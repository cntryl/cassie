# PostgreSQL 18 Delivery

This is the top-priority, dependency-ordered delivery record for
[#791](https://github.com/cntryl/cassie/issues/791) and the
[Postgres SQL feature parity milestone](https://github.com/cntryl/cassie/milestone/6).
The [selected dialect profile](sql-dialect-profile.md) defines full PostgreSQL
18.6 core SQL and PL/pgSQL scope and the only architectural exceptions.
The baseline is `769d4db8fb7e60f0a29fade73137433a20e5a817` (2026-10-10).
The initial inventory was 57 open and 12 closed; thirteen new owners #904–#916
extend it to 70 open and 12 closed before subsequent work. These are timestamped
planning counts, not completion evidence; refresh GitHub before later claims.

## Delivery stages

| Stage | Owners and order | Outcome |
| --- | --- | --- |
| 0 | #791–#793; #904–#916 | Scope, ownership, dependencies and bootstrap inventory/harness; retain #848 final gate. |
| 1 | #869 → #761; #768/#769; #819/#833; #904 | RETURNING/relational acceptance, codec/mutation qualification, isolation, transactional catalogs, shared type runtime. |
| 2 | #796, #798–#801, #803, #812–#816, #922/#923; #762 → #777 → #780 | Grammar/expressions/scalars; row grammar before row-subquery acceptance; catalog/client/wire foundations. |
| 3 | #804–#811; #817/#818; #820–#822; #905–#908 | Collections, aggregation, sessions, ranges/XML/geometry/money. |
| 4 | #823–#828, #834, #838–#841, #845/#846; #909 | Mutations/schema; constraints → table foundation → composite/domain integration. |
| 5 | #829–#832, #835–#837, #842–#844; #910–#916 | Full PL/pgSQL and remaining SQL objects, rules, statistics, local two-phase transactions and large objects. |
| 6 | #847/#848; #779 and prerequisites; #8 → #29 → #781; #849 | Complete client/dialect/cross-path/operational/restart acceptance on immutable revisions. |

Stages are grouping, not permission to bypass issue dependencies. #829 is
grouped with stage 5 programmability even though its JSON prerequisites can
prepare earlier. External prerequisites retain their existing milestones and
share this program's priority. Preserve completed foundations and finish only
their remaining acceptance obligations.

## New focused owners

| Owner | Outcome | Implementation prerequisites |
| --- | --- | --- |
| [#904](https://github.com/cntryl/cassie/issues/904) | Implement the shared PostgreSQL type and catalog runtime | #792, #793; extract oversized catalog modules before extending them |
| [#905](https://github.com/cntryl/cassie/issues/905) | Add PostgreSQL range and multirange types | #793, #904; #812 for numrange and #814 for tstzrange signatures only |
| [#906](https://github.com/cntryl/cassie/issues/906) | Add PostgreSQL XML values and SQL/XML functions | #793, #904 |
| [#907](https://github.com/cntryl/cassie/issues/907) | Add PostgreSQL geometric types and functions | #793, #904 |
| [#908](https://github.com/cntryl/cassie/issues/908) | Add PostgreSQL money values and arithmetic | #793, #904, #812 for numeric conversion signatures |
| [#909](https://github.com/cntryl/cassie/issues/909) | Implement PostgreSQL table inheritance | #826, #824, #833, #828 for named row-type integration |
| [#910](https://github.com/cntryl/cassie/issues/910) | Implement SQL-defined aggregate objects | #830, #804, #793, #904, #833 |
| [#911](https://github.com/cntryl/cassie/issues/911) | Implement SQL-defined casts and conversions | #830, #793, #904, #833 |
| [#912](https://github.com/cntryl/cassie/issues/912) | Implement SQL-defined operators and operator families | #830, #793, #904, #833 |
| [#913](https://github.com/cntryl/cassie/issues/913) | Implement PostgreSQL SQL rewrite rules | #833, #839, #823 for mutation actions |
| [#914](https://github.com/cntryl/cassie/issues/914) | Implement PostgreSQL extended statistics | #833, #842 ANALYZE implementation, #817 catalogs |
| [#915](https://github.com/cntryl/cassie/issues/915) | Implement local two-phase transactions and recovery | #819, #833, #793 durable format record, #820 locking |
| [#916](https://github.com/cntryl/cassie/issues/916) | Implement Midge-backed PostgreSQL large objects | #793 durable format record, #904, #819, #833, #831 privileges |
| [#922](https://github.com/cntryl/cassie/issues/922) | Match PostgreSQL planning-time constant expression errors | Shared expression/type/coercion and function-volatility contracts; keep runtime laziness distinct |
| [#923](https://github.com/cntryl/cassie/issues/923) | Avoid evaluating irrelevant EXISTS target expressions | Shared expression/type planning; pin target-elision contexts against PostgreSQL before implementation |

The two expression-planning owners were discovered by actual PostgreSQL 18.6
comparison while qualifying #869. They remain stage-2 runtime work and final
integration obligations for #761/#848; they do not invalidate the finite
RETURNING callback repair or become architectural exclusions. Its runtime
witnesses use row-dependent errors and PostgreSQL-qualified constraint ordering.

## Existing-owner extensions and staged dependencies

| Owner | Additional acceptance |
| --- | --- |
| #800 | TABLESAMPLE SYSTEM/BERNOULLI/REPEATABLE, explicit randomness assertions; complete row grammar before #762 row-subquery acceptance. |
| #816 | pg_lsn SQL value, OID/codecs/operators/functions; replication and physical PostgreSQL WAL administration remain excluded. |
| #824 | Exclusion constraints, PostgreSQL 18 temporal constraints, concurrent conflict enforcement; consume #905 built-in primitives. |
| #828 | Composite/record/domain only; range/multirange scope and all associated operators/functions/aggregates move to #905. |
| #837 | Event triggers in addition to row/statement/constraint/INSTEAD OF trigger execution. |
| #842 | Real Midge-backed VACUUM/ANALYZE/CHECKPOINT/REINDEX/CLUSTER effects, index/operator-family integration; accepting syntax alone cannot complete maintenance. |
| #835 | Full PL/pgSQL; the previous procedural subset is intermediate. |
| #793/#812/#807/#814 | Complete type records and final PostgreSQL numeric, array and temporal semantics; earlier finite limits are intermediate. |

Built-in ranges #905 depend on the #793 records and #904 registry, with #812
required only for numrange and #814 only for tstzrange signatures. They do not
depend on #824/#826/#828. Constraints then consume completed range primitives;
#826 follows constraints; #828 follows the table foundation. Custom-range
support functions and range/index integration are staged obligations under
#830/#912/#842, not blockers on unrelated built-in primitives. This breaks the
former range → constraint → table → composite cycle.

The [#925 structural prerequisite](https://github.com/cntryl/cassie/issues/925)
extracts projection virtual-catalog descriptors and row builders into the
private `src/catalog/virtual_views_projections.rs` module before #904 extends
the catalog runtime. Public entrypoints and database visibility stay in
`src/catalog/virtual_views.rs`. This extraction preserves existing behavior;
it does not implement the shared registry or qualify new SQL types, catalog
publication or durable formats. Record its merged commit and existing catalog,
database-scope and projection witnesses before consuming it as completed.

That prerequisite is accepted as [PR #926](https://github.com/cntryl/cassie/pull/926),
commit `2e7040dbce1f2235ba62ef2fbca52bee327a253c`: full ordered gate 3,845 Rust
passes, eight focused catalog/visibility/projection/wire witnesses and all 15
fresh hosted checks; exact merged tree verified, #925 closed. The next focused
[#928 bundle](https://github.com/cntryl/cassie/issues/928) centralizes metadata for
existing scalars and qualifies PostgreSQL Boolean binary reception. It changes
no public Rust signatures, legacy durable identities or mutable catalog roots;
#819/#833 remain open without being consumed by immutable builtin lookup.
Full #904/#793/#791 acceptance, named uint32 wire identities, all coercions,
new types and actual v3 upgrade round trips remain open. The independent native
ARM canonical failure stays failed under #927; its external remedy does not
become a circular prerequisite on unrelated static scalar primitives.

Record an implementation prerequisite as `owner; completed commit; exact tests;
passed result; remaining integration obligations` before dependent work starts.
A closed issue without that evidence is insufficient. #793 specifies contracts;
it does not depend on every type being implemented. Bootstrap #848 begins now;
its final gate still waits on every owner, client and operational obligation.

## Bundle and closeout policy

Reuse #877 for the revised profile/scope and bootstrap infrastructure, #878 for
the refreshed inventory ledger and #885 for the expanded type decision records.
Their current contents are useful foundations but do not complete the expanded
contracts. Integrate each coherent bundle serially after full validation/review.

For each runtime increment, retain red → green → refactor should_ tests and
Arrange/Act/Assert; async tests use a current-thread Tokio runtime. Run one heavy
local lane and preserve unrelated dirty work. Complete this ordered gate:

```sh
cargo build --locked
cargo test --locked
cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic
cargo fmt --all -- --check
cntryl-tools validate-tests -f <each touched test file>
cntryl-tools validate-docs --config .cntryl/repository.toml
cntryl-tools validate-benchmarks --config .cntryl/repository.toml
cntryl-tools check-module-sizes --config .cntryl/repository.toml
```

Then complete adversarial review, focused probes for Jev gap probability ≥0.30,
final-head hosted checks, squash merge and merged-commit/issue readback.
The milestone closes only after every in-scope corpus cell passes, every owner
and prerequisite acceptance criterion is complete and #849 resource/restart
evidence is accepted. Failed, skipped, unavailable, pending or unsupported
in-scope cells keep it open. Update support/readiness claims from that evidence
and close #791 last.

## Current primitive receive increment

The scalar registry and Boolean prerequisite is accepted as [PR #929](https://github.com/cntryl/cassie/pull/929),
commit `3077586b4d2719b9aaa15bbb095ed353c8eb0a24`, tree
`37622657c3416310efccc9eae1f3b95ba7ad2478`: full ordered gate 3,850 Rust
passes, twenty exact witnesses, 552 selected PostgreSQL comparisons, 24 additional
mixed-parameter comparisons and all fifteen fresh hosted checks. Exact merged
tree and #928 closure are verified; full parents remain open.

The next [#930 bundle](https://github.com/cntryl/cassie/issues/930) is defined
before coding. Existing scalar receive errors use the registered fixed width,
validate that prefix, then classify excess bytes as `22P03`; insufficient data
remains `08P01`. Confirmed invalid DATE/TIME/TIMESTAMP range errors use `22008`
before an excess-byte error. This is Stage 1 primitive work under #904/#780,
with explicit staged input-error ownership recorded in #814. It consumes the
completed #885 records, #926 extraction and #929 registry without consuming new
temporal types, mutable catalog roots or isolation. Both approved format
contracts and the currently admitted value set remain unchanged.

Read-only preparation retains all failures: the current 140-case scalar probe
has 120 passes and twenty failures; a 64-case temporal priority probe has
24 passes and forty failures. PostgreSQL validates an invalid temporal prefix
before checking excess bytes. Timestamp text presentation, infinity, 24:00,
large-year support and other full temporal obligations remain under #814/#904/#848.
Existing legacy-array elements delegate to the scalar receiver; #930 qualifies
the propagated error classes with explicit legacy identities and native oracle
probes. Array identity and grammar stay unchanged. The outer-container framing
change remains with #807 and its #796/#762 prerequisites.
The independent native ARM failure remains under #927. These are pending
program obligations, never architectural exceptions or passing cells.
