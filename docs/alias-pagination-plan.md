# Ordinary aliases and pagination integration plan

This 2026-10-07 plan defines one coherent candidate bundle for
[#795](https://github.com/cntryl/cassie/issues/795) and
[#797](https://github.com/cntryl/cassie/issues/797), under
[#791](https://github.com/cntryl/cassie/issues/791). It implements the selected
[D1/D2 contracts](sql-dialect-profile.md), without approving the remaining broad
#792/#793 proposals. [Feature Support](feature-support.md) owns support status.
This plan does not close either issue or qualify the candidate implementation.

## Dependency and implementation boundary

The lexical prerequisite is complete: #794 is closed and
[PR #852](https://github.com/cntryl/cassie/pull/852) was squash merged to
`3b03dd4db34ac88f533501d0484ecdc3cf223a2e` on 2026-10-07. The merged source
was read back and its diff against PR head
`b1039b6bdfbf2b9fbee0a4e6fcd96e33ba362880` is empty. That source patch was
integrated into the existing dirty pagination candidate before alias runtime work.
#797's selected prerequisite is D2, rather than broad #792. Independent scalar
integration remains owned by its existing lane. The joint candidate still needs
scalar visitor integration and the serial complete validation gates.

The accepted #794 scope excludes scalar dollar-quoted literals in both compact
and spaced expressions, repeated unary-sign chains beyond the admitted single
signed numeric literal, and chained/expanded postfix casts owned by #796.
Lexical protection of dollar-quoted interiors does not imply scalar literal
support. Aliases and bounds must not quietly expand these exclusions.

The local #797 candidate already uses `Option<Expr>` for public SELECT bounds
and logical-plan bounds. Its focused implementation evidence is distinct from
merged-main support and the still-pending joint full gates. The #795 candidate
now carries the selected public Aliased wrapper and private namespace lowering.
Focused red/green evidence covers ordinary forms, quoted aliases, prefix lists,
CTE wildcard row identity, self joins, parameter OIDs and actual indexed
parameterized pagination, cache context and underlying-table authorization.
Namespace, existing correlated SELECT/EXISTS and LATERAL scopes have focused
red/green witnesses. Public AST serde roundtrip and unaliased serialization checks
passed; the pgwire prefix/parameter portal probe passed with parameter OIDs
`[23,20,20]`, agreeing statement/portal descriptors, cumulative 2+1 rows and
42P10 excess-list errors. Final alias controls passed 12/12, including quoted
correlated qualifiers and inner shadowing. Existing SQL qualifier/join/CTE/recursive/
case and pagination regressions passed 71/71; parser AST/quoted/CTE checks passed
60/60; wire alias/pagination checks passed 2/2. These are focused local checks.
No merged #795 support or complete-gate result is claimed.

## Exact selected scope

D1 adds the public variant
`QuerySource::Aliased { source: Box<QuerySource>, alias: String, column_aliases: Vec<String> }`.
Admit ordinary Collection/Cte aliases, optional AS, schema-qualified base
relations, existing self-join forms and positional prefix column lists. Preserve
quoted spelling exactly; hide the original qualifier, reject unqualified
ambiguity, preserve the unrenamed suffix, and reject excess names with 42P10.
Binding, wildcard expansion, result labels and pgwire Describe/Execute must agree.
The underlying Collection remains the authorization, cache-context and physical
access-path identity. A base alias must not become a synthetic SELECT-star
Subquery. Existing unaliased enum serialization remains unchanged.

Qualified-star `r.*` is deferred by the owning #795 selection to a separate
projection contract. That approved exclusion permits finite #795 closure after
all other selected criteria, gates, review and merge readback are complete;
it does not permit a qualified-star support claim. Unqualified `*` is preserved.

Alias lowering preserves existing correlated WHERE/EXISTS and LATERAL scope
behavior, including inner shadowing and exact quoted qualifier spelling. The
existing binder rejects an outer reference in a nested JOIN ON clause even
without an ordinary alias: its ON reference check admits joined-source fields,
not the enclosing row. Matched unaliased and aliased controls retain that error;
this candidate does not introduce that additional correlation capability.
Duplicate positional column labels remain valid outputs (including wildcard
expansion); qualified or unqualified references with multiple matches reject
as ambiguous. Explicit ORDER BY output aliases use the existing projection
alias authority, including expression/function/window output labels.

D2 uses one authoritative `Option<Expr>` per LIMIT/OFFSET position, with existing
Expr serialization and zero-based parameters. Admit signed integers, NULL,
parentheses, parameters, checked integer +, - and *, and current
SMALLINT/INT/BIGINT casts. A bare unconstrained bound parameter is BIGINT/OID 20;
arithmetic operands need declared integer domains or individual explicit casts.
Unknown arithmetic rejects 42725; an outer result cast does not select its inner
operators. Omitted/LIMIT ALL is None, explicit NULL is Expr::Null; NULL LIMIT is
unbounded and NULL OFFSET is zero. Negative LIMIT/OFFSET reject 2201W/2201X and
overflow rejects 22003. Float arithmetic, division, functions, row references,
subqueries, aggregates, windows and volatile bounds remain excluded.

Resolve each bound once after binding, before cache/physical specialization or
source access, under existing admission/cancellation/deadline controls. Reuse the
evaluated bounds across scans, top-k, nested SELECTs, CTEs and cumulative portals;
do not allocate in proportion to the bound. Existing admitted mutation commands
with SELECT-bearing expressions require the same pre-mutation resolution.

## Positional aliases and Cassie's row identity

Use the ordered SQL-visible source shape, not the catalog field count, a
namespace HashSet, or internal row-key count. The authority is
[Reserved Identity Column](feature-support.md),
`binder::inference::relation_output_schema`, `binder::wildcard` and executor
wildcard projection. This applies D1 to the existing shape; it introduces no new
stored column or identity policy.

| Source | Ordered columns counted by the alias list | Example prefix result |
| --- | --- | --- |
| Base table `(score BIGINT, label TEXT)` without declared id | Leading implicit SQL `id`, then `score`, `label` | `r(key)` exposes `key`, `score`, `label`; `key` carries the existing identity value. |
| Base table `(id INT, score BIGINT)` with declared id | Declared `id`, `score` in schema order; reserved `_id` is not an extra wildcard column | `r(key)` exposes ordinary stored `key`, then `score`. |
| CTE or derived relation | Exactly the body projection, in projection order | No additional identity column is invented; a projected id is an ordinary exposed output. |

A prefix rename hides the renamed visible name in that relation namespace.
Internal `_id` storage and direct base-table reserved-identity access retain
their existing role; the reserved accessor is not an additional alias-list slot.
Preserve the distinction between a declared id and the legacy implicit id.
For example, a no-id table's two-column alias list over `(score BIGINT)` is
valid, while a third name rejects 42P10. The PostgreSQL preparation oracle used
a table with declared id and cannot prove this Cassie-specific count.

Visible names must not be used as a substitute for field identity. A renamed
ordinary column must not gain an identity optimization merely because its label
is `id` or `_id`; renaming the implicit identity must not lose its physical role.
Existing `should_return_a_value_for_a_column_aliased_to_the_internal_identity_name`
already proves `SELECT name AS _id` returns the ordinary projected value with
matching row/descriptor width. Preserve that behavior and the direct base `_id`
contract together. Cover name collisions with focused binder/output probes before
claiming alias support; if their resolution would change the existing reserved
identity contract, stop and record the precise conflict rather than choosing a
new public or storage policy. Do not omit this corner from the acceptance record.

## Source overlap and recovery evidence

The preserved clean `cassie-dialect-syntax` tree at
`356cd0cb69634e9179664165a20794ba1e2cd1cf` contains #794 commits, not a #795
implementation. Its ignored alias fixture failed its first explicit-AS case at
historical baseline `9ff42bc05c374f478a12a29caf7537268215e665`; later inputs in
that loop were not executed acceptance results. Its five operator greens and
later broader results do not establish alias greens. Preserve this recovery
tree and supersede its old unselected-wrapper proposal with approved D1.

The exact tracked #794 recovery overlap with the #797 candidate is parser
`expr.rs` and `tests/parser_types.rs`. Future D1/D2 overlap includes
`ast_query.rs`, `parser/query_select.rs`, binder query scopes and wildcard shapes,
planner/source walkers, physical specialization, executor source readers and
portal planning. A read-only inventory found 65 files using QuerySource, including
29 modified tracked candidate source files; this is a caller audit, not a mandate
to edit every file. New pagination source visitors must recurse through
`Aliased.source`, including their fallible admission traversal.

Keep ordinary alias parsing in a focused helper and namespace mapping in focused
binder/source modules. Existing binder/select.rs and sql/mod.rs are near their
1,000-line limits, so extract focused behavior before substantial additions.
Do not discard quoted alias identity through the current unconditional relation
qualifier folding in ColumnIdentifierPath. Audit callers that need visible
namespace separately from callers that need underlying Collection identity.

## Public and cache compatibility audit

The clean public query-AST break is explicitly selected for the next 0.2.x minor.
Legacy bound JSON may fail to decode; no QueryBound adapter, parallel legacy
fields or legacy-only decoder is selected. Coordinate version metadata with the
root-owned first public scalar bundle before publishing these changes. The #797
preparation checkout still has 0.1.0 metadata; this document neither bumps it nor
authorizes a release.

Current catalog FunctionMeta/ProcedureMeta bodies and ViewMeta queries retain SQL
strings. The catalog source audit found no stored ParsedStatement/QuerySource/
SelectStatement/Expr fields. Reconfirm the full persistent caller inventory when
adding D1; do not put the wrapper into a new durable catalog format.

CachedPlanEntry does persist PhysicalPlan with nested AST in StorageFamily::Temp.
[Database Families](database-families.md) excludes Temp from logical images:
these are rebuildable plans, not authoritative SQL/catalog/row data. The candidate
cost-model cache version is 4, replacing 3; the full PlanCacheKey contributes to
the persistent fingerprint. Old entries therefore miss the version-4 lookup and
the authoritative SQL is reparsed, bound and planned. Incompatible transient
deserialization is rejected as a miss rather than interpreted as a different
query. One version-4 epoch can cover the coherent D1/D2 shape changes. No durable
layout migration or source-data loss is selected. Preserve underlying relation
identity in authorization/cache dependency walks while retaining aliases in the
query's visible namespace and SQL shape.

## Implementation sequence and pending gates

1. Completed: read back #794 merge and closure and integrate its clean source
   patch into the dirty #797 candidate, preserving parser/test overlap. Final
   branch integration must retain that exact merged lexical source.
2. Completed: start #795 with focused failing `should_` tests and Arrange/Act/Assert: explicit
   and implicit aliases, schema qualification, quoted exactness, original-name
   hiding, self-join ambiguity, prefix/excess lists and ordered descriptors.
3. Completed in the local candidate: implement the wrapper and namespace mapping while preserving underlying
   identity. Exercise declared-id, implicit-id and CTE output shapes above;
   cover reserved-name output collisions and prevent phantom identity columns.
4. Audit all exhaustive AST matches and all source/permission/cache/planner/
   executor visitors. Eligible single Collection lowering retains indexed and
   ordered physical reads, with an actual accelerator probe. Joined alias
   wrappers keep distinct row qualifiers while a private Collection-identity
   helper retains already-admitted typed/hash and projected/indexed capabilities.
   Wrapper presence alone is not a fallback reason. Existing type, source-family,
   resource and kernel capability boundaries still decline by their existing
   laws. Scalar and relational visitors were first reconciled on prepared base
   `0211d09a653044f47d22c086b31ac97c9e20af7b`, then integrated with prepared
   storage/vector base `020fd29bbae9f263b48404542390360308bae947`. Final
   prerequisite test-hygiene integration and complete gates remain pending. Do not infer accelerator coverage from a
   generic fallback result.
5. Completed focused indexed/identity/cache/authorization and portal probes use
   `SELECT r.key, r.score FROM records AS r(key) ORDER BY r.score, r._id LIMIT $1 OFFSET $2`
   against a no-declared-id indexed table. Check BIGINT descriptors, actual identity
   values, exact names/rows and the eligible ordered read path. Repeat the namespace
   cases with quoted aliases, a declared-id table, CTE scopes and cumulative
   pgwire portal execution. Negative bounds must fail before source access or an
   admitted UPDATE/DELETE mutation; pagination walkers must see aliased nested
   sources even when a shallow bound already needs resolution.
6. Preserve #797's focused serializer/AST, NULL/error/overflow, typed-parameter,
   cached-plan, direct-executor, subquery/CTE, resource-boundary and portal evidence.
   Resolve any confirmed cross-slice regression with its own small red/green
   increment, then update canonical support/docs only to the proven finite scope.
7. Once the coherent candidate is ready, run required gates once in order:
   `cargo build`, `cargo test --locked`,
   `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`,
   `cargo fmt --all -- --check`, then
   `cntryl-tools validate-tests -f <path>` for each touched test file. Record exact
   commands and failures; no complete gate has run on the joint candidate yet.
8. Open one PR with the selected exclusions and individual issue criteria,
   perform Jev/adversarial review, address concrete gaps, verify final-head hosted
   checks, squash merge, and read back both finite issue closures and merged source.

Jev's recorded preparation judgment favored the joint bundle after #794 merge
(0.77, confidence 0.69), with the underlying physical identity/visible namespace
plus parameterized pagination probe selected as the key overlap. Request/response
evidence is retained under ignored `target/alias-preparation/`. This supports the
conditional plan, not runtime correctness, contract approval or closure. If #795
cannot satisfy its finite criteria, keep its issue open and record the blocker;
the grouping is not permission to bypass it.

## Candidate evidence and handoff (2026-10-07)

The implementation preserves parsed public Aliased wrappers. Private binding
lowers eligible single Collection queries to physical fields, while joined,
CTE and correlated namespaces retain distinct encoded row qualifiers. The
underlying Collection remains intact for physical reads, permission checks and
cache dependency identity. Genuine red/green increments cover original-name
hiding, prefix ambiguity, CTE wildcard identity, joined descriptors, alias
parameter OIDs, column/expression ORDER output precedence and existing
correlated/LATERAL scopes. The actual indexed identity+pagination probe also
verifies cache behavior and reader authorization against the underlying table.

Jev selected the initial physical identity probe with probability 0.65 and
nested scope review with choice probability 0.94 (gap 0.72). Those choices
produced the focused indexed and existing correlated probes and confirmed
fixes. Final scope/quoted adversarial probabilities 0.63/0.40 led to matched
unsupported JOIN ON controls, exact quoted correlated aliases, inner shadowing
and the existing 71 SQL/60 parser sibling checks. These are triage judgments,
not correctness or closure proof. Ignored target artifacts retain the exact
requests, responses and red/green logs.

The integration owner must reconcile scalar/conditional/cache and relational
source visitors, retain the selected public 0.2.x metadata and cache epoch 4
compatibility disposition, then run the standing full gates in order. Required
complete build/tests/pedantic clippy/fmt/test-policy, exact final-head review,
publication, merge and issue readback remain pending. Private integration commits preserve the reviewed candidate; no PR or
publication is created by this handoff.

## Private integration acceptance on 2026-10-07

The D1/D2 candidate integrates prepared scalar/conditional/cache visitors and
#758/#759 first on private base `0211d09a653044f47d22c086b31ac97c9e20af7b`,
then prepared #771 storage and #776 vector source on
`020fd29bbae9f263b48404542390360308bae947`. Complete
binding still precedes one conditional normalization pass; scalar-generated
origin markers, cache semantic epoch 4 and the selected public 0.2 boundary
remain intact. Publication still requires prerequisite bundle merge/readback
and the integration owner's complete ordered gates.

Four focused shared-boundary probes pass: scalar origin/conditional normalization
through aliased CTEs and evaluated bounds; ordinary and genuinely renamed prefix
aliases on the admitted typed-hash path against disabled-native and unaliased
controls; qualified ORDER BY versus an output-label collision on the same actual
ordered index path; and exact quoted physical payload projection with BIGINT
descriptors on that index path. The last three include genuine failing evidence
before their private identity/canonical-field/namespace handoff fixes.

Jev selected preservation of already-admitted Collection capabilities with
probability 1.0 and assigned quoted-field handoff risk 0.74. The required quoted
probe exposed double quoting in binder validation and missing runtime aliases
for physically projected rows; both now pass without changing stored fields or
SQL-visible names. Qualified-star syntax remains the owning #795 exclusion.
The pre-existing unsupported outer reference inside a nested JOIN ON remains
a matched alias/unaliased decline; this slice adds no correlation or kernel
family beyond the selected contracts.

The carrier collision probe initially returned the aliased table's payload for
both qualifiers when an unaliased table was literally named
`__cassie_relation_alias_72`. Binding now allocates a collision-free private
carrier before source reference lowering. Parsed public `Aliased` wrappers and
underlying Collection identity remain intact. The allocator collects names from
sources, JOIN ON, nested EXISTS/derived queries, table-function arguments,
projection/window expressions, filter/group/having/order/bounds, CTE definitions
and set operands. It preserves inner scope shadowing and chooses a name absent
from both visible and encoded namespaces. There is no reserved SQL prefix.
This early pass rewrites namespace ownership only; catalog field and prefix-list
validation still occur after authoritative source/search-path binding.

Quoted/case-distinct controls admit carrier-like table and LATERAL aliases,
verify distinct payload ownership, inner shadowing, ORDER and evaluated bounds.
Jev favored private scoped allocation (0.99) and assigned CTE traversal risk
0.51; the required ordinary/quoted CTE collision probe passes. The existing
joined-outer EXISTS reference boundary remains a matched alias/unaliased
decline; positive correlated ownership uses the already-admitted LATERAL path.

On the prepared vector base, the focused shared SQL run passes 106 tests;
the final alias integration run passes all seven probes, including CTE carrier
collisions. Earlier integrated checks passed 60 parser siblings, two pgwire
alias/pagination probes and the actual projected typed-scan unit probe. Exact
red/green logs and Jev requests/responses remain in ignored
`target/alias-preparation/`. Final prerequisite hygiene integration and repeated
focused checks on the immutable head remain pending, as do parent-owned build,
full tests, pedantic Clippy, format check and touched-test validation. Local
probes are not publication or closure proof.
