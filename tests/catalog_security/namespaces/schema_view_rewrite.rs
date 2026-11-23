//! `ALTER SCHEMA ... RENAME` rewrites a stored view definition only where a
//! schema qualifier stands, never inside literals or at aliases.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::Cassie;
use cassie::types::Value;

fn schema_a(label: &str, view: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE SCHEMA a",
            "CREATE TABLE a.t (id INT)",
            "INSERT INTO a.t VALUES (1)",
            view,
        ],
    )
}

fn renamed_rows(label: &str, view: &str) -> Vec<Vec<Value>> {
    let fixture = schema_a(label, view);
    fixture.execute("ALTER SCHEMA a RENAME TO b").unwrap();
    fixture.rows("SELECT * FROM b.vv")
}

#[test]
fn should_keep_a_string_literal_with_an_alias_while_renaming_a_schema() {
    // Arrange
    let view = "CREATE VIEW a.vv AS SELECT 'a.txt' AS lit, data.id FROM a.t AS data";

    // Act
    let rows = renamed_rows("schema_rename_literal_alias", view);

    // Assert
    assert_eq!(
        rows,
        vec![vec![Value::String("a.txt".into()), Value::Int64(1)]]
    );
}

#[test]
fn should_rename_an_uppercase_unquoted_schema_qualifier() {
    // Arrange
    let view = "CREATE VIEW a.vv AS SELECT A.t.id FROM A.t";

    // Act
    let rows = renamed_rows("schema_rename_uppercase_qualifier", view);

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_keep_escape_string_text_with_dollar_quote_text_while_renaming_a_schema() {
    // Arrange
    let view = "CREATE VIEW a.vv AS SELECT E'\\'a.x' AS e, $$a.y$$ AS d, t.id FROM a.t";

    // Act
    let rows = renamed_rows("schema_rename_escape_dollar", view);

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::String("'a.x".into()),
            Value::String("a.y".into()),
            Value::Int64(1)
        ]]
    );
}

#[test]
fn should_keep_an_alias_named_like_the_schema() {
    // Arrange
    let view = "CREATE VIEW a.vv AS SELECT a.id FROM a.t AS a";

    // Act
    let rows = renamed_rows("schema_rename_alias_is_schema", view);

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_keep_a_literal_in_a_materialized_view_while_renaming_a_schema() {
    // Arrange
    let view = "CREATE MATERIALIZED VIEW a.vv AS SELECT 'a.txt' AS lit, data.id FROM a.t AS data";

    // Act
    let rows = renamed_rows("schema_rename_materialized", view);

    // Assert
    assert_eq!(
        rows,
        vec![vec![Value::String("a.txt".into()), Value::Int64(1)]]
    );
}

#[test]
fn should_rewrite_an_external_view_when_its_source_schema_is_renamed() {
    // Arrange
    let fixture = sql_fixture(
        "schema_rename_external_view",
        &[
            "CREATE SCHEMA a",
            "CREATE SCHEMA x",
            "CREATE TABLE a.t (id INT)",
            "INSERT INTO a.t VALUES (5)",
            "CREATE VIEW x.vv AS SELECT id FROM a.t",
        ],
    );

    // Act
    fixture.execute("ALTER SCHEMA a RENAME TO b").unwrap();
    let rows = fixture.rows("SELECT * FROM x.vv");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(5)]]);
}

#[test]
fn should_update_external_view_text_to_the_renamed_schema() {
    // Arrange
    let fixture = sql_fixture(
        "schema_rename_external_view_text",
        &[
            "CREATE SCHEMA a",
            "CREATE SCHEMA x",
            "CREATE TABLE a.t (id INT)",
            "CREATE VIEW x.vv AS SELECT id FROM a.t",
        ],
    );

    // Act
    fixture.execute("ALTER SCHEMA a RENAME TO b").unwrap();
    let stored = fixture
        .cassie
        .catalog
        .get_view("x.vv")
        .expect("view metadata");

    // Assert
    assert!(stored.query.contains("b.t"));
}

#[test]
fn should_rewrite_persisted_function_scope_when_a_schema_is_renamed() {
    // Arrange
    let fixture = sql_fixture(
        "schema_rename_view_function_scope",
        &[
            "CREATE SCHEMA fn_source",
            "CREATE SCHEMA fn_caller",
            "CREATE FUNCTION fn_source.same_name() RETURNS INT AS '41'",
            "SET search_path = fn_source, public",
            "CREATE VIEW fn_caller.scoped_view AS SELECT same_name() AS result",
            "SET search_path = fn_caller, public",
        ],
    );

    // Act
    fixture
        .execute("ALTER SCHEMA fn_source RENAME TO fn_renamed")
        .expect("rename function source schema");
    let stored = fixture
        .cassie
        .catalog
        .get_view("fn_caller.scoped_view")
        .expect("view metadata");
    let rows = fixture.rows("SELECT result FROM fn_caller.scoped_view");

    // Assert
    assert_eq!(
        stored.function_dependency_search_path,
        ["fn_renamed", "public"]
    );
    assert_eq!(rows, vec![vec![Value::Int64(41)]]);
}

struct SchemaViewDirectory(String);

impl Drop for SchemaViewDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove closed schema-view fixture");
    }
}

#[test]
fn should_preserve_rewritten_view_function_scope_after_restart() {
    // Arrange
    crate::support_sql::use_local_storage();
    let directory =
        SchemaViewDirectory(crate::support_sql::data_dir("schema-view-function-restart"));
    {
        let cassie = Cassie::new_with_data_dir(&directory.0).expect("create initial Cassie");
        cassie.startup().expect("initialize initial Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE SCHEMA restart_fn_source",
            "CREATE SCHEMA restart_view_owner",
            "CREATE FUNCTION restart_fn_source.same_name() RETURNS INT AS '51'",
            "SET search_path = restart_fn_source, public",
            "CREATE VIEW restart_view_owner.scoped_view AS SELECT same_name() AS result",
            "ALTER SCHEMA restart_fn_source RENAME TO restart_fn_renamed",
        ] {
            cassie
                .execute_sql(&session, sql, vec![])
                .expect("create renamed-schema fixture");
        }
        cassie.shutdown();
    }

    let cassie = Cassie::new_with_data_dir(&directory.0).expect("reopen Cassie");
    cassie.startup().expect("restore renamed view scope");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "SET search_path = restart_view_owner, public",
            vec![],
        )
        .expect("change caller schema");

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT result FROM restart_view_owner.scoped_view",
            vec![],
        )
        .expect("execute restored cross-schema view");

    // Assert
    assert_eq!(result.rows(), &[vec![Value::Int64(51)]]);
    cassie.shutdown();
}

#[test]
fn should_preserve_literals_in_an_external_view_when_its_source_schema_is_renamed() {
    // Arrange
    let fixture = sql_fixture(
        "schema_rename_external_literal",
        &[
            "CREATE SCHEMA a",
            "CREATE SCHEMA x",
            "CREATE TABLE a.t (id INT)",
            "INSERT INTO a.t VALUES (5)",
            "CREATE VIEW x.vv AS SELECT 'a.t' AS label, id FROM a.t",
        ],
    );

    // Act
    fixture.execute("ALTER SCHEMA a RENAME TO b").unwrap();
    let rows = fixture.rows("SELECT * FROM x.vv");
    let stored = fixture
        .cassie
        .catalog
        .get_view("x.vv")
        .expect("view metadata");

    // Assert
    assert_eq!(
        rows,
        vec![vec![Value::String("a.t".into()), Value::Int64(5)]]
    );
    assert!(stored.query.contains("'a.t'"));
    assert!(stored.query.contains("b.t"));
}

#[test]
fn should_leave_an_unrelated_view_definition_unchanged_when_a_schema_is_renamed() {
    // Arrange
    let fixture = sql_fixture(
        "schema_rename_unrelated_view",
        &[
            "CREATE SCHEMA a",
            "CREATE SCHEMA z",
            "CREATE TABLE a.t (id INT)",
            "CREATE TABLE z.source (id INT)",
            "CREATE VIEW z.vv AS SELECT id FROM z.source",
        ],
    );
    let before = fixture
        .cassie
        .catalog
        .get_view("z.vv")
        .expect("view metadata")
        .query;

    // Act
    fixture.execute("ALTER SCHEMA a RENAME TO b").unwrap();
    let after = fixture
        .cassie
        .catalog
        .get_view("z.vv")
        .expect("view metadata")
        .query;

    // Assert
    assert_eq!(after, before);
}
