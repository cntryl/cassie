# Relational semantics qualification

This finite qualification slice tracks [#761](https://github.com/cntryl/cassie/issues/761)
and the confirmed correlated SELECT output defect
[#864](https://github.com/cntryl/cassie/issues/864). Qualification is in progress;
the selected public grammar, type identities and support levels remain the
contracts in [the SQL dialect profile](sql-dialect-profile.md).

## Selected correlated output repair

Classify each `EXISTS` occurrence independently using the existing binder's
inner/outer scope authority. A successful bind without outer fields remains
uncorrelated and is folded before source execution. When that bind fails but
binding the same statement against the admitted enclosing source fields
succeeds, only that occurrence is deferred to the actual outer row. If both
binds fail, preserve the original error. This classification performs no
subquery execution and uses the same CTE scope, parameters and session context.

Uncorrelated siblings still fold before source execution, including when the
outer input is empty or another sibling is correlated. Deferred SELECT output
occurrences resolve per actual row after filtering, grouping, windows and
ordering, before projection, DISTINCT and final pagination. This remains a
scalar expression boundary and does not promote aliases or correlation to a
native kernel. Fresh AST, field, lookup and row copies must be admitted before
allocation and retain the existing parent/query/operator owners through their
handoff. Cancellation and errors release temporary owners. The source attachment
copy has its own pre-allocation lease even while the original outer row remains
live. Deferred correlated output uses the existing scalar evaluator's lazy CASE
and COALESCE selection through a private borrowed resolver; only reached nodes
execute, and conditions/arguments are evaluated once. Static uncorrelated
folding, including dead-branch and empty-input errors, remains unchanged.

Qualified references reserve their actual parsed namespaces during private
carrier allocation, including nested scopes. Alias lowering preserves an
inner carrier only when the enclosing admitted namespace would collide with
the underlying collection qualifier. An exact binder-supplied outer field
may pass alias lowering only when no inner namespace claims its qualifier.

The slice preserves existing uncorrelated and WHERE behavior. CASE, supported
function wrappers, inner shadowing, CTE scope, empty input, grouped output,
errors and owner retirement require concrete controls. HAVING, ORDER BY and
join ON behavior is not silently moved to a different evaluation phase; any
unsupported sibling discovered by qualification receives an explicit finding.

## Evidence status

The 14 seed tables have passed actual PostgreSQL-wire parameter OID and stored
carrier readback, including NULL arrays, negative zero and integers above
2^53. Literal query rows and descriptors are separate from implementation
helpers. All 22 primary literal cases and their Statement Describe/text and
binary Portal descriptors now pass focused qualification. The confirmed
three-shape correlated SELECT defect has actual red/green evidence, with
WHERE, inner shadow, CTE, CASE/function and eager empty-input/sibling controls.
The original failed attempts and preparation corrections remain retained;
this is not a completed qualification or a full-suite pass. Independent copy
ownership, threshold-minus-one denial, cancellation and final-reader retirement
now pass focused controls, including warmed lookups with spare vector capacity.
Selected CASE/COALESCE dead branches, reached errors, dynamic Boolean parameters
and a session-local volatile CASE operand pass; scalar callbacks run only for
reached nodes and preserve the independently parsed function-body error boundary.
Developer quality and a coherent implementation checkpoint remain separate from
completion of the remaining qualification matrix and full validation.

Completion still requires all 22 primary invariants, concrete positive and
negative variants, pagination parameters, rich keys, frames, operator path
witnesses, selected PostgreSQL 18 oracle records, and normal required local
and exact-head hosted validation.

## SQL-006 preparation correction

The positive variant originally assumed a positional alias list on a derived
subquery, `FROM (SELECT n,a FROM r WHERE id=3) AS q(x)`. The current subquery
parser treats the whole suffix as its alias; actual execution rejects `q.x`
with SQLSTATE 42601. D1 selected prefix lists for ordinary Collection/Cte aliases,
so this is an unsupported preparation input, not a regression or a request to
extend the grammar. Preserve that failed attempt. The corrected variant uses
`WITH c AS (SELECT n,a FROM r WHERE id=3) SELECT q.x,q.a FROM c AS q(x)`, with the
same independent NULL rows and exact descriptors. The primary derived-source
case keeps its supported inner projection alias `n AS x` and ordinary `AS q`.

The corrected CTE prefix variant preserves output labels `x` and `a`: binder
`aliases::lower_select` sets the projected alias from `ColumnIdentifierPath::declared_name`;
`projection::compile_projection_ops` uses that explicit alias. The original
prepared `q.x`/`q.a` metadata described the unchanged derived primary, not this
CTE variant. All twenty variants executed successfully before this metadata
assertion failed; the failed descriptor attempt remains retained.

## Finite scalar frame qualification

The independent frame records use four ordered BIGINT rows `(1,10)`,
`(2,20)`, `(3,20)`, `(4,30)`. Peer groups are `[10]`, `[20,20]`,
and `[30]`; endpoint expectations for GROUPS offsets and all three EXCLUDE
forms are materialized before execution. Both statements retain the selected
scalar frame path; this adds no syntax or native-path promotion. Embedded rows
and text/binary Statement/Portal descriptors must match those literals.

## Negative record scope

The alias-arity negative is `WITH c AS (SELECT id FROM r) SELECT q.x FROM c AS q(x,y)`: one source field and two prefix aliases must reject under the selected CTE law. The earlier derived-list draft is unsupported preparation, preserved separately. Ordinal bounds, incompatible CASE outputs, recursive arity/types, and D2 bound errors retain their existing parser/binder/wire authorities; no rejection is treated as a request to add syntax.

## Completed-path witness scope

Scoped caller-thread subscribers retain every relational completion event, not
a single last-path slot. Direct primitive sort/top-k/distinct/set/distinct-on
and ranking windows must report their selected admitted paths. Expression
ordering, CTE DISTINCT, and GROUPS frames must retain their named scalar
boundaries. A composed DISTINCT then sort query requires both events. These
witnesses do not infer worker coverage or private input handoff from an event;
those are separate controls. No global subscriber is installed.

## D2 repeated Bind qualification

For one prepared query per text/binary format, rebind limits/offsets `2/1`,
`1/2`, `0/0`, `NULL/0`, and `2/NULL` without reparsing the statement between
binds. The fixed ordered eligible ids are `[5,1,2,6]`; exact results are
`[1,2]`, `[2]`, `[]`, `[5,1,2,6]`, and `[5,1]`. Initial parameter Describe
advertises BIGINT/BIGINT, Statement Describe is text, and every Portal Describe
and DataRow preserves its Bind format. No stale bound or portal result may leak.

## Focused evidence-gap probes

Jev bounded review scored the preparation-oracle question 0.36 and scoped-event
attribution question 0.39. The selected probes verify quoted CTE prefix labels
`X`/`a` and BIGINT/ARRAY descriptors on NULL-only and empty inputs, and two
concurrent independent caller subscribers with a barrier: primitive DISTINCT
versus scalar GROUPS window. Neither subscriber may record the other owner.
These probes do not assert async worker coverage or whole-matrix acceptance.

## Private owner-partition qualification

DISTINCT consumes real batches. A 1,025-row declared BIGINT fixture repeats
`NULL,1,2,3,0` classes and splits at 0, 1, 512, 1,023, 1,024, and 1,025.
Literal first occurrences are `[NULL,1,2,3,0]`; each nonempty input partition
owns a separate 256-byte source lease marker. Winners retain their actual source
markers until output drop, and every marker and operator charge then retires.

Set kernels consume complete row vectors. The existing adapter boundary receives
partition-owned rows before flattening; no streaming set API is selected.
The 4x5x3 source-partition grid for `sa UNION (sb INTERSECT sc)` preserves the
independent bag `[NULL,1,2]`, with `sa=[1,1,NULL]`, `sb=[1,2,NULL,NULL]`,
and `sc=[2,NULL]`. The four two-branch operators separately preserve their
recorded bags. Input partitions include empty endpoints, declared types and
source lease markers; output ownership and final zero charge are observed.
Marker reservations witness existing source-owner retention, not an estimate of
all test-fixture allocation. No runtime operator or public API is added.

## Finite PostgreSQL and homogeneous join qualification

Installed PostgreSQL 18.6 runs in an owned UUID cluster, UTF8/C-libc/UTC,
with a private loopback endpoint. Forty-five preselected row comparisons passed:
19 compatible primary cases, 20 variants, two homogeneous BIGINT join controls,
two frames and two quoted-prefix NULL/empty cases. Unsupported Cassie SQL-002
and original mixed exact-number join cases remain excluded; ARRAY OID and
SUM/AVG descriptor parity are not inferred. Binary-derived FLOAT seed negative
zero remains exactly `8000000000000000`. The first metadata-only attempt failed
before any case because `lc_collate` is database catalog metadata, not a PG18
SHOW parameter; that failed attempt and both strict cleanup receipts are retained.

The homogeneous Cassie control uses independently enumerated seven LEFT and
five INNER pairs with vectorized joins disabled and enabled. Actual unaliased
LEFT joins report `vectorized`; INNER joins report `typed_hash`. Both report
build4/probe5/matched5; disabled controls report merge with zero vectorized build.
The legacy semantic-key LEFT diagnostic reports four non-NULL retained build
rows as `right_input_rows_total`; typed INNER and merge report all five right
source rows. Ordinary aliases preserve the same literal rows and `id`/`id`
BIGINT descriptors in both wire formats. These are case-specific actual path
witnesses, not native eligibility inferred from a configuration flag.

Initial prepared metadata, blanket native/fallback attribution and physical
right-input counter assumptions failed against current source. Those attempts
remain preserved separately from runtime red/green evidence. The LEFT typed
eligibility checks actual right-row/template entry and alias agreement before
conversion; declining that check retains the existing semantic-key path. No
runtime kernel, metadata rule, metrics implementation or public API changes.

## Final selected split controls

The next finite controls preserve the existing complete-input join adapter and
window batch entrypoints. Homogeneous five-row sides receive all 6x6 source
partition boundaries before their admitted flattening; independent LEFT/INNER
pairs and actual typed completion remain required. The original six-row window
seed receives splits 0..6, singleton batches and `[2,1,2,1]` chunks, with literal
rank/dense-rank outputs and declared types. Separate source markers witness
retention and final retirement; no streaming join API or shared scalar primary
oracle is introduced. Existing typed join NULL/output-budget controls, rich
DISTINCT fallback and recursive depth/resource controls supply their precise
selected ownership checks when actually replayed on the refreshed source.

The same final snapshot adds ordered private mixed-numeric DISTINCT literals
(raw negative-zero first winner, exact large integer versus rounded float) and
declared ARRAY NULL/empty/element-NULL identity across every batch split. These
retain the existing semantic-key fallback and source owner until output drop.
The review's quoted literal-dot column candidate first checks ordinary SELECT
and WHERE EXISTS before correlated SELECT, using identical two-row inputs;
unsupported baseline syntax is not treated as an invitation to extend grammar.

## Selected delimited-field correlation repair

[Bug #866](https://github.com/cntryl/cassie/issues/866) records actual ordinary
quoted-field SELECT success with matching WHERE/SELECT EXISTS scope errors.
A literal stored component such as `a.b` must be distinguished from an encoded
qualified path using current source/schema/alias provenance and existing
`ColumnIdentifierPath` authority, preserving the original WHERE phase and
per-occurrence SELECT folding. Collection fields, selected CTE prefix renames
and derived projection labels are probed separately before choosing the private
naming seam. No punctuation-wide rewrite, new grammar, public API or per-row
schema clone is selected. The original unsupported derived positional alias-list
boundary remains unchanged. All current owner/cancellation and 2 KiB WHERE
controls remain required.

The finite #866 seam selects exact declared Collection fields, explicit derived
projection aliases, unqualified ColumnIdentifierPath output components, and
wildcard fields inherited from a single declared Collection (through ordinary
aliases). Successful CTE-prefix remapping and supported derived `q.id` paths
remain unchanged. The attempted outer `q."r.id"` reference to an unaliased
qualified derived projection is unsupported even in ordinary SELECT and is
recorded as a preparation error, not a grammar expansion. Literal raw names are
escaped through the existing identifier helper before the current qualifier is
prepended in static SELECT scope, dynamic SELECT rows and original WHERE rows.

Current focused #866 regressions retain ordinary, WHERE and SELECT results for
quoted dotted fields, escaped quotes, exact uppercase fields, successful CTE
prefix remapping and the selected derived source shapes. The final literal
inner-priority probe yields `true,false`; distinct `a.b`/`A.B` fields yield
`false,true` in each row, with BOOLEAN descriptors. These answer the concrete
Jev 0.74/0.64 candidates through actual tests, rather than model approval.
The 34-test relational suite and unchanged 2 KiB WHERE control pass locally;
three scoped-copy and four attachment admission/cancellation controls retain
their owners and reject the observed threshold-minus-one budgets. The private
join/window partition controls also pass. This is a focused checkpoint:
combined joined-source/writer-barrier qualification, required full validation
and exact-head hosted acceptance remain pending before issue closeout.
