# Role access contract

## Selected v0.2.0 profile

Cassie authenticates each network session as one fixed login role. Non-root
roles use the existing password-backed role records and must have CONNECT on
the selected database. `root` remains the administrator and bypasses object
privileges. `SET ROLE`, role membership, `PUBLIC` grants, grant options,
ownership changes, row-level security, routines, sequences and default
privileges are outside this profile and reject deterministically.

The selected privileges are database CONNECT, schema USAGE, and table SELECT,
INSERT, UPDATE and DELETE. A GRANT or REVOKE names one role and one schema or
table target; one statement may list several of the selected privileges for
that target. Multi-target statements and unselected privilege names reject
before publication. Database CONNECT remains in `RoleMeta`.

Object grants use separate typed grant records in Cassie's existing Midge
Schema-family metadata. A record identifies one grantee role, the database's
stable physical family, one privilege target and its selected privilege set.
Use stable role, namespace and relation identities where the catalog already
provides them; names remain display values. Updating one role/target grant is a
single-record Midge write. It does not span database families or add a storage
abstraction. Startup hydrates the records into the in-memory catalog; GRANT and
REVOKE update that catalog only after the Midge commit succeeds.

Direct GRANT and REVOKE statements are supported outside explicit SQL
transactions. They reject inside an explicit transaction with SQLSTATE 25001
until the ACL record can join the selected transactional catalog publication.
Dropping a granted schema or table outside a transaction removes its records
from the Schema family. Inside a transaction, DDL rollback preserves the grant.
After successful database-family DDL publication, Cassie deletes the removed
targets' grant records in an idempotent Schema-family transaction. This cleanup
is postcommit housekeeping, not part of the atomic database-family commit. If
the process stops between publication and cleanup, the records are inert because
authorization requires the stable target to exist; startup reconciles records
whose targets are absent. The lifecycle regressions
[`should_remove_table_grants_only_after_transactional_drop_commits`](../tests/catalog_security/security/object_privilege_grants/lifecycle.rs),
[`should_remove_cascaded_object_grants_only_after_schema_drop_commits`](../tests/catalog_security/security/object_privilege_grants/lifecycle.rs),
and [`should_reconcile_stale_table_grants_after_restart`](../tests/catalog_security/security/object_privilege_grants/lifecycle.rs)
exercise rollback, commit, and restart reconciliation through the selected
`DROP TABLE`/`DROP SCHEMA` forms in #833's DDL inventory. They qualify only the
grant-cleanup boundary and do not qualify #833's broader acceptance.

The role authenticated at connection startup remains the authorization
principal for the session. Direct grants are checked at statement admission,
including prepared Bind/Execute, pgwire COPY and every supported query or DML
path. Schema USAGE is required to resolve relations in that schema. A table
operation requires its matching table privilege. SELECT is also required when
the statement reads existing target-column values: UPDATE or DELETE predicates,
assignments, sources, subqueries and returned target columns; INSERT query
sources, data-modifying CTEs, named ON CONFLICT target columns, constraints,
expressions or conditions, and returned target columns; and MERGE join/WHEN
conditions, expressions or returned target columns. MERGE with a DO NOTHING
action also requires target SELECT. COPY FROM requires INSERT and COPY TO
requires SELECT on its query sources. A constant UPDATE without a row predicate
and an unfiltered DELETE can use only their matching operation privilege.
Separate query sources still require SELECT on each source table. Root-only
administrative statements continue to require the administrator role.

Selected `has_database_privilege`, `has_schema_privilege` and
`has_table_privilege` calls report CONNECT, USAGE and SELECT/INSERT/UPDATE/DELETE
for the authenticated session role. Trusted embedded sessions use their
existing internal administrator context for two-argument calls; explicit-role
signatures require a persisted role. Unsupported privilege names do not return
a fabricated success. Comma-separated supported table privileges return true
when the role holds at least one requested privilege, matching
[PostgreSQL 18](https://www.postgresql.org/docs/18/functions-info.html).
`pg_namespace.nspacl`
and `pg_class.relacl` remain experimental TEXT carriers and report only direct
selected grants in a PostgreSQL-readable ACL string: schema USAGE is `U`,
table SELECT/INSERT/UPDATE/DELETE is `r`, `a`, `w`, `d`, and `root` is the
grantor. Entries are sorted by grantee and privileges use `arwd` order; no
direct grants returns SQL NULL.
These fields do not claim PostgreSQL ACLITEM[] descriptor compatibility,
default/PUBLIC grants or inherited grants.

The selected `has_*` argument forms use role and object names. OID-valued role
or object arguments are outside this slice and reject deterministically. Jev
selected the finite named-only forms with probability and confidence 1.0; the
source-backed question and response are retained under ignored
`target/v020-evidence/831-role-access/has-privilege-argument-scope-request.json`
and `target/v020-evidence/831-role-access/has-privilege-argument-scope-response.json`.
The focused rejection regression is authored but unexecuted.

Grant identity follows stable object identity across rename. Dropping a role,
schema, table or database removes its grant records from the Schema family;
transactional schema/table drops clean up only after successful database-family
publication. Grant records for a missing identity never authorize a later
object. Restarting the same store restores its current grant state. Database
image export/import does not transfer these shared Schema-family role grants;
that behavior is outside #831 unless a separate contract selects it. No
backward compatibility or migration guarantee between unreleased builds is
required.

## Jev decision record

Question: which durable grant carrier best balances atomic single-family
updates, bounded record size, and truthful rename/drop/restart behavior for
#831? Jev selected separate grant records in the existing Schema family with
probability 0.93 (choice confidence 0.91); the alternatives were ACL maps on
`RoleMeta` (0.06) and object-local metadata in database families (0.01).

The choice is consistent with the source boundary: `RoleMeta` already stores
authentication and CONNECT state as one record, and `Midge::put_role` commits
that record through the Schema family. Separate grant records keep the role
credential record bounded while using the same published Midge transaction
surface. Source review and executable lifecycle regressions determine the
implementation; Jev's result is not proof. The exact question and response are
retained under ignored `target/v020-evidence/831-role-access/`.

Transaction question: should direct grants and dropped-object cleanup inside
explicit SQL transactions be atomic with the SQL DDL publication, rejected, or
committed independently? Jev selected rejecting direct GRANT/REVOKE inside an
explicit transaction and tying drop cleanup to #833 commit/rollback with
probability 0.71 and confidence 0.61 (transactional integration 0.28,
immediate commit 0.00, none 0.01). The `immediate_commit` option was rejected
because rollback could leave access state inconsistent. The exact question and
response are retained under ignored `target/v020-evidence/831-role-access/`.

Storage-boundary follow-up: after source inspection showed that Midge's
transaction scope rejects schema-family plus database-family transactions,
Jev selected retaining Schema-family grant records and performing idempotent
postcommit cleanup with probability 0.75 and confidence 0.67. Stale rows are
inert because stable dropped-object identities are not reused; startup
reconciliation removes them. Moving grant rows into each database family was
the alternative (0.21), but would broaden the selected storage contract and
hydration path. This does not claim grant deletion shares the DDL's atomic
database-family commit. The exact question and response are retained under
ignored `target/v020-evidence/831-role-access/`.

Privilege-read question: should every UPDATE/DELETE require target SELECT, or
only statements that read existing target values? Jev selected requiring the
matching write privilege plus target SELECT when the statement references
existing target values (probability 0.97, confidence 0.96). PostgreSQL 18's
Privileges documentation describes SELECT as necessary when existing column
values are referenced by UPDATE/DELETE/MERGE; its INSERT and MERGE references
also enumerate target-column read cases for ON CONFLICT and MERGE. We use those
references to delimit the finite table-level rule, then verify source and
regressions; Jev's judgment is not proof. The exact question and response are
retained under ignored `target/v020-evidence/831-role-access/`.

Related-path questions: Jev selected target SELECT when ON CONFLICT or MERGE
reads target values, with MERGE DO NOTHING also requiring SELECT (probabilities
0.96 and 0.98; confidences 0.95 and 0.97). The PostgreSQL 18 references are
[`INSERT`](https://www.postgresql.org/docs/18/sql-insert.html),
[`MERGE`](https://www.postgresql.org/docs/18/sql-merge.html), and
[`Privileges`](https://www.postgresql.org/docs/18/ddl-priv.html). The exact
question and response are retained under ignored
`target/v020-evidence/831-role-access/`.

Sequence-helper boundary: the current statement gate accepted `SELECT` and
recursed through function arguments without checking the called function, so a
non-root login could reach sequence helpers despite sequences being outside the
selected profile. Jev selected rejecting `nextval`, `currval`, `setval`,
`lastval`, and `pg_get_serial_sequence` for non-root authenticated sessions
(probability 1.0, confidence 1.0), while preserving root behavior. The guard
uses the SQL function registry's typed `FunctionId` values and returns the
selected `42501` privilege error. The regression in
[`object_privilege_grants.rs`](../tests/catalog_security/security/object_privilege_grants.rs)
covers all five helpers and root `nextval`; it is authored but unexecuted. The
exact request and response are retained under ignored
`target/v020-evidence/831-role-access/sequence-scope-{request,response}.json`.

Sequence-default boundary: a follow-up source review found that sequence
defaults execute outside the SQL expression tree. The non-root statement gate
now rejects INSERT, `INSERT ... ON CONFLICT`, `MERGE ... INSERT`, COPY FROM,
UPDATE, and `MERGE ... UPDATE` paths when an omitted/defaulted target column
would consume a serial, identity, or explicit `nextval` default. It also
rejects `OVERRIDING USER VALUE` for identity columns. Explicit values remain
allowed when they supply every sequence-backed target value without an
override that consumes the sequence. The focused regressions
[`should_reject_implicit_sequence_defaults_for_non_admin_insert`](../tests/catalog_security/security/object_privilege_grants.rs)
and
[`should_reject_other_non_admin_writes_that_consume_sequence_defaults`](../tests/catalog_security/security/object_privilege_grants.rs)
are authored but unexecuted.

Database-image scope: Jev selected documenting same-store restart only with
probability and confidence 1.0. The issue requires restart durability and does
not require transferring grants through database-image import; the current
import path copies a database's physical family while role grants live in the
shared Schema family. The contract now states that image transfer is outside
#831 until separately specified. The exact question and response are retained
under `target/v020-evidence/831-role-access/database-image-scope.{request,response}.json`.

Focused adversarial review: Jev selected four source-backed boundaries with
probability and confidence 1.0 each: reject every non-root path that consumes
a sequence-backed default while allowing explicit values that avoid sequence
use; delete role metadata and grants atomically in one Schema-family
transaction; perform postcommit object-drop grant cleanup with startup
reconciliation because DDL and grants live in different Midge families; and
reject PUBLIC grants. The corresponding request/response are retained under
`target/v020-evidence/831-role-access/role-access-adversarial-{request,response}.json`.
These judgments are checked against the current authorization/storage source
and the named authored regressions; all probes remain unexecuted.
