# Query-engine delivery tracks (historical)

> **Superseded.** This 2026-10-09 assignment snapshot was written for the former
> PostgreSQL 18 full-parity program. It is retained for delivery provenance only;
> it does not define current goals, owners, dependencies, or parallel-work policy.
> Follow the [Product Roadmap](product-roadmap.md) and live GitHub milestones.

These ownership tracks organize the dependency-ordered backlog under
[#747](https://github.com/cntryl/cassie/issues/747) and
[#791](https://github.com/cntryl/cassie/issues/791). They do not change the finite
runtime contracts, support status, production readiness, or issue acceptance
criteria. Live GitHub issue state and dependencies determine which slice may
start; closed issues retain their ownership here for delivery provenance.

## Ownership

The assignment inventory was refreshed on 2026-10-09. Each issue has one primary
track; shared contracts and cross-track edits belong to integration.

| Track | Assigned issues | Primary boundary |
| --- | --- | --- |
| Relational semantics | #761, #762, #864, #866, #868, #870, #800, #801, #804, #805, #806, #808, #811, #829, #834 | Scope, correlated expressions and relational operator semantics |
| Transaction and authorization ownership | #763, #766, #767, #819, #820, #821, #822 | Statement Data visibility, transaction/session owners and cache authorization |
| Retrieval correctness | #861, #764, #765, #775 | ANN, text/hybrid retrieval and time-series qualification |
| Physical execution | #768, #773, #774, #778 | Layouts/codecs, projections, portable kernels and execution diagnostics |
| SQL expressions and types | #796, #798, #799, #803, #807, #809, #810, #812, #813, #814, #815, #816 | Scalar syntax, values, functions and type semantics |
| Catalog and mutation lifecycle | #769, #823, #824, #825, #826, #827, #828, #830, #831, #832, #833, #835, #836, #837, #838, #839, #840, #841, #842, #843, #845, #869 | Constraints, mutation/RETURNING, durable object identity, DDL and routines |
| PostgreSQL clients and compatibility | #777, #780, #817, #818, #844, #846, #847 | Wire/session clients, finite catalog read models and migration interoperability |
| Acceptance and operations | #779, #8, #29, #781, #848, #849 | Cross-path acceptance, retained native profiles and release qualification |
| Integration and contracts | #747, #791, #792, #793 | Shared contract decisions, bundle acceptance and serial publication/merge |

Catalog lifecycle owns durable object changes; client compatibility owns their
finite catalog read models. Integration reviews that interface. Mutation
RETURNING owns its evaluation phase while relational semantics owns reusable
EXISTS scope machinery. Neither track independently expands the other's contract.

## Activation gates

- Relational qualification precedes bounded scalar subqueries: #761 then #762,
  followed by #777 and #780. The selected SQL-022 repairs #868/#869/#870 are
  accepted as recorded below; #761 remains open for its full qualification.
- Statement visibility #763 precedes #764, #766, #768, #769, #773 and #775.
  ANN qualification #764 precedes text/hybrid #765; cache/auth #766 precedes
  REST-vector #767. These lists do not replace additional named prerequisites.
- #774 follows completed #760. Diagnostics #778 follows #768/#775/#773/#774;
  cross-path #779 follows #769/#766/#778.
- Broad dialect runtime expansion follows profile #792 and type/wire/storage
  contract #793. Only explicitly selected finite existing-contract exceptions
  may proceed earlier; their issue and dialect-profile records remain authority.
- Operations #8 follows its correctness/compatibility dependencies, then #29
  and release consolidation #781. Dialect corpus #848 and resource/restart #849
  retain their full issue dependency lists. Evidence preparation is not promotion.

## Parallel work and serial integration

Use three bounded implementation worktrees and one integration owner. Assign
shared-file ownership before code. Keep distinct build directories, storage paths
and server ports; only one heavy full Cargo validation runs on the local host.
Workers may prepare independent ready slices. Blocked tracks may prepare source
inventories and reviewable contract decisions without implementing blocked behavior.

The initial first wave paired relational #761/#864/#866 review with #763
statement-view review and separate #861 attribution. The finite #763 Data-family
statement-view and overlay prerequisite is accepted through [PR #871](https://github.com/cntryl/cassie/pull/871),
commit `ce72a23f2d0a3cdb9a811107aec7629151fcd319`. The selected SQL-022 repair
prerequisites for #761 are also accepted:

| Owner / completing PR | Immutable merge commit | Finite acceptance evidence |
| --- | --- | --- |
| #870 / [#872](https://github.com/cntryl/cassie/pull/872) | `700cbad7bc87801b51eceb0b532451d0dc021d1b` | Nested ancestor scope, including `ancestor_scope::should_preserve_ancestor_scope_in_nested_exists`; final focused run passed four unit and five relational tests. |
| #868 / [#873](https://github.com/cntryl/cassie/pull/873) | `3ccb302c4d6b9f97dc033e90a53c2930e663086c` | Correlated EXISTS in HAVING, ordering, JOIN ON and window values, including `exists_having::should_resolve_exists_against_actual_having_groups`; final relational suite passed 57 tests. |
| #869 / [#884](https://github.com/cntryl/cassie/pull/884) | `4a35dc373c4ac449184d42001055b447b3c53dc4` | RETURNING statement view, row phase and atomicity, including `exists_returning::should_resolve_update_returning_exists_against_captured_view_with_new_row`; 20 RETURNING integration and ten private resource tests passed in the final full gate and as separate retained witnesses. |

The named cases belong to `tests/relational_qualification.rs`; linked PRs retain
complete validation, review and exact merged-tree/issue-closure evidence. These
finite repairs do not close #761 or complete later scalar-subquery, coercion,
client or wire acceptance. The known #861 fixture ownership repair is already
on main; it does not establish the prolonged wait's cause.

Each runtime behavior follows local red -> green -> refactor. Publish one PR at
a time after its required ordered local gates. Complete clean adversarial review,
mandatory focused probes and actual repository-required GitHub rules/checks on
the final head before guarded squash merge and issue/source readback. Under the
user-selected local-first policy, disclose pending nonrequired hosted checks;
they need not delay merge and must not be reported as passed. The
[closeout policy](postgresql18-delivery.md#bundle-and-closeout-policy) retains the
complete local and final acceptance gates. Rebase remaining worktrees after each
accepted merge.
Keep persistent-format, migration and public-contract decisions explicit and
approved before dependent implementation.
