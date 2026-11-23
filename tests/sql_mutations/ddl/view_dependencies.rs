//! Dependent views are found from the parsed view definition, so `DROP VIEW`,
//! `DROP TABLE` and `ALTER TABLE ... DROP COLUMN` agree with PostgreSQL 18.6
//! (SQLSTATE 2BP01) however the view spells its sources and column references.

use cassie::app::Cassie;
use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn relation_count(fixture: &SqlFixture, names: &str) -> Vec<Vec<Value>> {
    fixture.rows(&format!(
        "SELECT count(*) FROM pg_class WHERE relname IN ({names})"
    ))
}

fn chain_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE b3 (id INT PRIMARY KEY)",
            "CREATE VIEW dv1 AS SELECT * FROM b3",
            "CREATE VIEW dv2 AS SELECT * FROM dv1",
        ],
    )
}

#[test]
fn should_reject_dropping_a_view_another_view_reads() {
    // Arrange
    let fixture = chain_fixture("view-dep-drop-view");

    // Act
    let sqlstate = fixture.error("DROP VIEW dv1").sql_state().to_owned();
    let remaining = relation_count(&fixture, "'dv1', 'dv2'");

    // Assert
    assert_eq!(sqlstate, "2BP01");
    assert_eq!(remaining, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_drop_a_view_list_that_contains_the_dependent_view() {
    // Arrange
    let fixture = chain_fixture("view-dep-drop-view-list");

    // Act
    let dropped = fixture.execute("DROP VIEW dv1, dv2");
    let remaining = relation_count(&fixture, "'dv1', 'dv2'");

    // Assert
    assert!(dropped.is_ok(), "both views go in one statement");
    assert_eq!(remaining, vec![vec![Value::Int64(0)]]);
}

#[test]
fn should_drop_every_view_in_the_chain_with_drop_view_cascade() {
    // Arrange
    let fixture = chain_fixture("view-dep-drop-view-cascade");
    fixture
        .execute("CREATE VIEW dv3 AS SELECT * FROM dv2")
        .expect("the third view is created");

    // Act
    let dropped = fixture.execute("DROP VIEW dv1 CASCADE");
    let remaining = relation_count(&fixture, "'dv1', 'dv2', 'dv3'");
    let table_rows = fixture.rows("SELECT count(*) FROM b3");

    // Assert
    assert!(dropped.is_ok(), "CASCADE drops the dependent views");
    assert_eq!(remaining, vec![vec![Value::Int64(0)]]);
    assert_eq!(table_rows, vec![vec![Value::Int64(0)]]);
}

fn two_table_fixture(label: &str, view: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE va (id INT PRIMARY KEY)",
            "CREATE TABLE vt (id INT PRIMARY KEY)",
            view,
        ],
    )
}

fn drop_table_sqlstate(label: &str, view: &str) -> String {
    two_table_fixture(label, view)
        .error("DROP TABLE vt")
        .sql_state()
        .to_owned()
}

#[test]
fn should_reject_dropping_a_table_a_view_reads_through_a_comma_join() {
    // Arrange
    let view = "CREATE VIEW vc AS SELECT va.id AS a_id, vt.id AS t_id FROM va, vt";

    // Act
    let sqlstate = drop_table_sqlstate("view-dep-comma-join", view);

    // Assert
    assert_eq!(sqlstate, "2BP01");
}

#[test]
fn should_reject_dropping_a_table_a_view_reads_in_a_subquery() {
    // Arrange
    let view = "CREATE VIEW vs AS SELECT id FROM va WHERE id IN (SELECT id FROM vt)";

    // Act
    let sqlstate = drop_table_sqlstate("view-dep-subquery", view);

    // Assert
    assert_eq!(sqlstate, "2BP01");
}

#[test]
fn should_reject_dropping_a_table_a_view_reads_in_a_cte() {
    // Arrange
    let view = "CREATE VIEW vw AS WITH c AS (SELECT id FROM vt) SELECT id FROM c";

    // Act
    let sqlstate = drop_table_sqlstate("view-dep-cte", view);

    // Assert
    assert_eq!(sqlstate, "2BP01");
}

#[test]
fn should_reject_dropping_a_table_a_view_reads_through_an_explicit_join() {
    // Arrange
    let view = "CREATE VIEW vj AS SELECT va.id FROM va LEFT JOIN vt ON va.id = vt.id";

    // Act
    let sqlstate = drop_table_sqlstate("view-dep-explicit-join", view);

    // Assert
    assert_eq!(sqlstate, "2BP01");
}

#[test]
fn should_drop_the_whole_view_chain_with_drop_table_cascade() {
    // Arrange
    let fixture = sql_fixture(
        "view-dep-table-cascade-chain",
        &[
            "CREATE TABLE t (id INT PRIMARY KEY)",
            "CREATE VIEW cv1 AS SELECT id FROM t",
            "CREATE VIEW cv2 AS SELECT id FROM cv1",
        ],
    );

    // Act
    let dropped = fixture.execute("DROP TABLE t CASCADE");
    let remaining = relation_count(&fixture, "'t', 'cv1', 'cv2'");

    // Assert
    assert!(dropped.is_ok(), "CASCADE drops the table");
    assert_eq!(remaining, vec![vec![Value::Int64(0)]]);
}

fn column_fixture(label: &str, view: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE tc (id INT PRIMARY KEY, val INT, other INT)",
            "CREATE TABLE tz (id INT PRIMARY KEY, val INT)",
            view,
        ],
    )
}

fn drop_column_sqlstate(label: &str, view: &str, column: &str) -> String {
    column_fixture(label, view)
        .error(&format!("ALTER TABLE tc DROP COLUMN {column}"))
        .sql_state()
        .to_owned()
}

#[test]
fn should_reject_dropping_a_column_a_view_filters_on() {
    // Arrange
    let view = "CREATE VIEW vcol AS SELECT id FROM tc WHERE val>0";

    // Act
    let sqlstate = drop_column_sqlstate("view-dep-column-filter", view, "val");

    // Assert
    assert_eq!(sqlstate, "2BP01");
}

#[test]
fn should_reject_dropping_a_column_a_view_selects_with_a_table_qualifier() {
    // Arrange
    let view = "CREATE VIEW vcol AS SELECT tc.other FROM tc";

    // Act
    let sqlstate = drop_column_sqlstate("view-dep-column-qualified", view, "other");

    // Assert
    assert_eq!(sqlstate, "2BP01");
}

#[test]
fn should_reject_dropping_a_column_a_view_selects_through_a_wildcard() {
    // Arrange
    let view = "CREATE VIEW vcol AS SELECT * FROM tc";

    // Act
    let sqlstate = drop_column_sqlstate("view-dep-column-wildcard", view, "val");

    // Assert
    assert_eq!(sqlstate, "2BP01");
}

#[test]
fn should_reject_dropping_a_column_a_view_uses_through_an_alias() {
    // Arrange
    let view = "CREATE VIEW vcol AS SELECT x.id FROM tc AS x WHERE x.val IS NOT NULL";

    // Act
    let sqlstate = drop_column_sqlstate("view-dep-column-alias", view, "val");

    // Assert
    assert_eq!(sqlstate, "2BP01");
}

#[test]
fn should_reject_dropping_a_column_a_view_uses_in_a_correlated_subquery() {
    // Arrange
    let view = "CREATE VIEW vcol AS SELECT id FROM tz \
                WHERE EXISTS (SELECT 1 FROM tc WHERE tc.val = tz.id)";

    // Act
    let sqlstate = drop_column_sqlstate("view-dep-column-subquery", view, "val");

    // Assert
    assert_eq!(sqlstate, "2BP01");
}

#[test]
fn should_drop_a_column_a_view_only_names_in_a_string_literal() {
    // Arrange
    let fixture = column_fixture(
        "view-dep-column-literal",
        "CREATE VIEW vcol AS SELECT id, 'val' AS label FROM tc",
    );

    // Act
    let dropped = fixture.execute("ALTER TABLE tc DROP COLUMN val");

    // Assert
    assert!(
        dropped.is_ok(),
        "a string literal is not a column reference"
    );
}

#[test]
fn should_drop_a_column_a_view_only_names_as_an_output_alias() {
    // Arrange
    let fixture = column_fixture(
        "view-dep-column-output-alias",
        "CREATE VIEW vcol AS SELECT id AS val FROM tc ORDER BY val",
    );

    // Act
    let dropped = fixture.execute("ALTER TABLE tc DROP COLUMN val");

    // Assert
    assert!(dropped.is_ok(), "an output alias is not a column reference");
}

#[test]
fn should_drop_a_column_a_view_reads_from_another_table() {
    // Arrange
    let fixture = column_fixture(
        "view-dep-column-other-table",
        "CREATE VIEW vcol AS SELECT tc.id, tz.val FROM tc, tz",
    );

    // Act
    let dropped = fixture.execute("ALTER TABLE tc DROP COLUMN val");

    // Assert
    assert!(dropped.is_ok(), "tz.val is not a column of tc");
}

#[test]
fn should_drop_the_view_chain_with_drop_column_cascade() {
    // Arrange
    let fixture = column_fixture(
        "view-dep-column-cascade-chain",
        "CREATE VIEW cv1 AS SELECT id, val FROM tc",
    );
    fixture
        .execute("CREATE VIEW cv2 AS SELECT id FROM cv1")
        .expect("the second view is created");

    // Act
    let dropped = fixture.execute("ALTER TABLE tc DROP COLUMN val CASCADE");
    let remaining = relation_count(&fixture, "'cv1', 'cv2'");

    // Assert
    assert!(dropped.is_ok(), "CASCADE drops the column");
    assert_eq!(remaining, vec![vec![Value::Int64(0)]]);
}

#[test]
fn should_reject_dropping_a_function_used_by_a_view_chain() {
    // Arrange
    let fixture = sql_fixture(
        "view-dep-function-drop",
        &[
            "CREATE TABLE function_source (a INT)",
            "INSERT INTO function_source VALUES (1)",
            "CREATE FUNCTION view_increment(x INT) RETURNS INT AS 'x + 1'",
            "CREATE VIEW function_view AS \
             SELECT view_increment(a) AS result FROM function_source",
            "CREATE VIEW function_view_child AS SELECT result FROM function_view",
        ],
    );

    // Act
    let error = fixture.error("DROP FUNCTION IF EXISTS view_increment");
    let function_result = fixture.rows("SELECT view_increment(1)");
    let view_result = fixture.rows("SELECT result FROM function_view_child");

    // Assert
    assert_eq!(error.sql_state(), "2BP01");
    assert!(
        matches!(
            error,
            cassie::app::CassieError::Diagnostic(ref diagnostic)
                if diagnostic.detail.as_deref().is_some_and(
                    |detail| detail.contains("function_view depends on function view_increment")
                )
        ),
        "the detail names the directly dependent view"
    );
    assert_eq!(function_result, vec![vec![Value::Int64(2)]]);
    assert_eq!(view_result, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_drop_a_function_without_dependent_views() {
    // Arrange
    let fixture = sql_fixture(
        "view-dep-function-unused",
        &["CREATE FUNCTION unused_increment(x INT) RETURNS INT AS 'x + 1'"],
    );

    // Act
    let dropped = fixture.execute("DROP FUNCTION unused_increment");

    // Assert
    assert!(dropped.is_ok());
}

#[test]
fn should_preserve_the_view_creation_search_path_for_function_dependencies() {
    // Arrange
    let fixture = sql_fixture(
        "view-dep-function-search-path",
        &[
            "CREATE SCHEMA selected_functions",
            "CREATE SCHEMA shadow_functions",
            "CREATE FUNCTION selected_functions.same_name(x INT) RETURNS INT AS 'x + 1'",
            "CREATE FUNCTION shadow_functions.same_name(x INT) RETURNS INT AS 'x + 2'",
            "SET search_path = selected_functions, shadow_functions, public",
            "CREATE VIEW search_path_view AS SELECT same_name(1) AS result",
            "SET search_path = public",
        ],
    );

    // Act
    let unrelated_drop = fixture.execute("DROP FUNCTION shadow_functions.same_name");
    let dependent_drop = fixture.error("DROP FUNCTION selected_functions.same_name");
    let value = fixture.rows("SELECT result FROM search_path_view");

    // Assert
    assert!(unrelated_drop.is_ok());
    assert_eq!(dependent_drop.sql_state(), "2BP01");
    assert_eq!(value, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_execute_a_view_with_its_persisted_function_search_path() {
    // Arrange
    let fixture = sql_fixture(
        "view-function-execution-search-path",
        &[
            "CREATE SCHEMA selected_execution",
            "CREATE SCHEMA caller_execution",
            "CREATE FUNCTION selected_execution.same_name() RETURNS INT AS '11'",
            "CREATE FUNCTION caller_execution.same_name() RETURNS INT AS '22'",
            "SET search_path = selected_execution, public",
            "CREATE VIEW execution_context_view AS SELECT same_name() AS result",
            "SET search_path = caller_execution, public",
        ],
    );

    // Act
    let value = fixture.rows("SELECT result FROM execution_context_view");

    // Assert
    assert_eq!(value, vec![vec![Value::Int64(11)]]);
}

#[test]
fn should_preserve_nested_view_function_scope_during_exists_rebinding() {
    // Arrange
    let fixture = sql_fixture(
        "view-nested-function-exists-scope",
        &[
            "CREATE SCHEMA selected_nested",
            "CREATE SCHEMA caller_nested",
            "CREATE FUNCTION selected_nested.same_name(x INT) RETURNS BOOL AS 'x = 7'",
            "CREATE FUNCTION caller_nested.same_name(x INT) RETURNS BOOL AS 'x = 9'",
            "SET search_path = selected_nested, public",
            "CREATE VIEW nested_function_view AS \
             SELECT EXISTS (SELECT 1 WHERE same_name(7)) AS matched",
            "SET search_path = caller_nested, public",
        ],
    );

    // Act
    let value = fixture.rows("SELECT matched FROM nested_function_view");

    // Assert
    assert_eq!(value, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_preserve_view_scope_for_scalar_derived_and_having_phases() {
    // Arrange
    let fixture = sql_fixture(
        "view-function-phase-scope",
        &[
            "CREATE TABLE phase_source (id INT)",
            "INSERT INTO phase_source VALUES (1), (2)",
            "CREATE SCHEMA selected_phases",
            "CREATE SCHEMA caller_phases",
            "CREATE FUNCTION selected_phases.keep_value(x INT) RETURNS BOOL AS 'x = 2'",
            "CREATE FUNCTION caller_phases.keep_value(x INT) RETURNS BOOL AS 'x = 1'",
            "SET search_path = selected_phases, public",
            "CREATE VIEW phase_scope_view AS \
             SELECT id FROM (SELECT id FROM phase_source) AS derived \
             WHERE (SELECT keep_value(MAX(id)) FROM phase_source) \
             GROUP BY id HAVING keep_value(id)",
            "SET search_path = caller_phases, public",
        ],
    );

    // Act
    let rows = fixture.rows("SELECT id FROM phase_scope_view");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(2)]]);
}

struct ViewFunctionDirectory(String);

impl Drop for ViewFunctionDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove closed view-function fixture");
    }
}

#[test]
fn should_preserve_view_function_resolution_after_restart() {
    // Arrange
    crate::support_sql::use_local_storage();
    let directory = ViewFunctionDirectory(crate::support_sql::data_dir("view-function-restart"));
    {
        let cassie = Cassie::new_with_data_dir(&directory.0).expect("create initial Cassie");
        cassie.startup().expect("initialize initial Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE SCHEMA restart_selected",
            "CREATE SCHEMA restart_caller",
            "CREATE FUNCTION restart_selected.same_name() RETURNS INT AS '31'",
            "CREATE FUNCTION restart_caller.same_name() RETURNS INT AS '32'",
            "SET search_path = restart_selected, public",
            "CREATE VIEW restart_context_view AS SELECT same_name() AS result",
        ] {
            cassie
                .execute_sql(&session, sql, vec![])
                .expect("create restart fixture");
        }
        cassie.shutdown();
    }

    let cassie = Cassie::new_with_data_dir(&directory.0).expect("reopen Cassie");
    cassie
        .startup()
        .expect("restore view function dependencies");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "SET search_path = restart_caller, public", vec![])
        .expect("change caller function search path");

    // Act
    let result = cassie
        .execute_sql(&session, "SELECT result FROM restart_context_view", vec![])
        .expect("execute restored view");

    // Assert
    assert_eq!(result.rows(), &[vec![Value::Int64(31)]]);
    cassie.shutdown();
}

#[test]
fn should_reject_drop_function_cascade_without_deleting_dependents() {
    // Arrange
    let fixture = sql_fixture(
        "view-dep-function-cascade",
        &[
            "CREATE FUNCTION cascade_increment(x INT) RETURNS INT AS 'x + 1'",
            "CREATE VIEW cascade_view AS SELECT cascade_increment(1) AS result",
        ],
    );

    // Act
    let error = fixture.error("DROP FUNCTION cascade_increment CASCADE");
    let view_remains = fixture
        .rows("SELECT result FROM cascade_view")
        .first()
        .cloned();

    // Assert
    assert_eq!(error.sql_state(), "0A000");
    assert_eq!(view_remains, Some(vec![Value::Int64(2)]));
}

#[test]
fn should_reject_duplicate_create_view_output_names() {
    // Arrange
    let fixture = sql_fixture(
        "view-duplicate-output",
        &["CREATE TABLE duplicate_output_source (a INT)"],
    );

    // Act
    let aliased = fixture.error("CREATE VIEW duplicate_alias AS SELECT 1 AS a, 2 AS a");
    let projected = fixture
        .error("CREATE VIEW duplicate_projected AS SELECT a, a FROM duplicate_output_source");
    let folded = fixture.error("CREATE VIEW duplicate_folded AS SELECT 1 AS A, 2 AS a");
    let remaining = relation_count(
        &fixture,
        "'duplicate_alias', 'duplicate_projected', 'duplicate_folded'",
    );

    // Assert
    assert_eq!(aliased.sql_state(), "42701");
    assert_eq!(projected.sql_state(), "42701");
    assert_eq!(folded.sql_state(), "42701");
    assert_eq!(remaining, vec![vec![Value::Int64(0)]]);
}

#[test]
fn should_accept_distinct_create_view_output_names() {
    // Arrange
    let fixture = sql_fixture("view-distinct-output", &[]);

    // Act
    let created = fixture.execute("CREATE VIEW distinct_output AS SELECT 1 AS a, 2 AS b");

    // Assert
    assert!(created.is_ok());
}

#[test]
fn should_apply_a_create_view_column_list_in_order() {
    // Arrange
    let fixture = sql_fixture(
        "view-output-column-list",
        &[
            "CREATE TABLE column_list_source (a INT, b INT)",
            "INSERT INTO column_list_source VALUES (1, 2)",
        ],
    );

    // Act
    let created =
        fixture.execute("CREATE VIEW listed_view(p, q) AS SELECT a, b FROM column_list_source");
    let values = fixture.rows("SELECT p, q FROM listed_view");
    let names = fixture.rows(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_name = 'listed_view' ORDER BY ordinal_position",
    );

    // Assert
    assert!(created.is_ok());
    assert_eq!(values, vec![vec![Value::Int64(1), Value::Int64(2)]]);
    assert_eq!(
        names,
        vec![
            vec![Value::String("p".into())],
            vec![Value::String("q".into())]
        ]
    );
}

#[test]
fn should_preserve_unlisted_create_view_output_names() {
    // Arrange
    let fixture = sql_fixture(
        "view-short-output-column-list",
        &["CREATE TABLE short_column_list_source (a INT)"],
    );

    // Act
    let created = fixture.execute(
        "CREATE VIEW short_list_view(p) AS \
         SELECT a, a + 1 AS b FROM short_column_list_source",
    );
    let names = fixture.rows(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_name = 'short_list_view' ORDER BY ordinal_position",
    );

    // Assert
    assert!(created.is_ok());
    assert_eq!(
        names,
        vec![
            vec![Value::String("p".into())],
            vec![Value::String("b".into())]
        ]
    );
}

#[test]
fn should_honor_quoted_create_view_column_names() {
    // Arrange
    let fixture = sql_fixture("view-quoted-output-column-list", &[]);

    // Act
    let created = fixture.execute("CREATE VIEW quoted_list_view(\"P\", q) AS SELECT 1, 2");
    let names = fixture.rows(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_name = 'quoted_list_view' ORDER BY ordinal_position",
    );

    // Assert
    assert!(created.is_ok());
    assert_eq!(
        names,
        vec![
            vec![Value::String("P".into())],
            vec![Value::String("q".into())]
        ]
    );
}

#[test]
fn should_ignore_parentheses_inside_quoted_create_view_identifiers() {
    // Arrange
    let fixture = sql_fixture("view-quoted-parentheses", &[]);

    // Act
    let created_view =
        fixture.execute("CREATE VIEW \"quoted(view)\"(\"left(part)\", q) AS SELECT 1, 2");
    let created_without_list = fixture.execute("CREATE VIEW \"plain(view)\" AS SELECT 3 AS value");
    let names = fixture.rows(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_name = 'quoted(view)' ORDER BY ordinal_position",
    );
    let values = fixture.rows("SELECT \"left(part)\", q FROM \"quoted(view)\"");

    // Assert
    assert!(created_view.is_ok());
    assert!(created_without_list.is_ok());
    assert_eq!(
        names,
        vec![
            vec![Value::String("left(part)".into())],
            vec![Value::String("q".into())]
        ]
    );
    assert_eq!(values, vec![vec![Value::Int64(1), Value::Int64(2)]]);
}

#[test]
fn should_reject_an_oversized_create_view_column_list() {
    // Arrange
    let fixture = sql_fixture("view-oversized-output-column-list", &[]);

    // Act
    let error = fixture.error("CREATE VIEW oversized_list_view(p, q, r) AS SELECT 1 AS a, 2 AS b");
    let remaining = relation_count(&fixture, "'oversized_list_view'");

    // Assert
    assert_eq!(error.sql_state(), "42601");
    assert!(error
        .to_string()
        .contains("CREATE VIEW specifies more column names than columns"));
    assert_eq!(remaining, vec![vec![Value::Int64(0)]]);
}

#[test]
fn should_reject_duplicate_ctas_output_names() {
    // Arrange
    let fixture = sql_fixture("view-ctas-duplicate-output", &[]);

    // Act
    let error = fixture.error("CREATE TABLE duplicate_ctas AS SELECT 1 AS a, 2 AS a");
    let remaining = relation_count(&fixture, "'duplicate_ctas'");

    // Assert
    assert_eq!(error.sql_state(), "42701");
    assert_eq!(remaining, vec![vec![Value::Int64(0)]]);
}
