// Integration suite: EXISTS and NOT EXISTS subqueries in every clause and
// statement that accepts a boolean expression.

#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/sql_fixture.rs"]
mod support_sql_fixture;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

mod exists_in_dml {
    use cassie::types::Value;

    use crate::support_sql_fixture::{sql_fixture, SqlFixture};

    fn dml_fixture(label: &str) -> SqlFixture {
        sql_fixture(
            label,
            &[
                "CREATE TABLE t (a INT, b TEXT)",
                "CREATE TABLE empty_t (z INT)",
                "CREATE TABLE one_t (z INT)",
                "INSERT INTO t (a, b) VALUES (1, 'p')",
                "INSERT INTO t (a, b) VALUES (2, 'q')",
                "INSERT INTO t (a, b) VALUES (3, 'r')",
                "INSERT INTO one_t (z) VALUES (1)",
            ],
        )
    }

    fn remaining(fixture: &SqlFixture) -> Vec<Vec<Value>> {
        fixture.rows("SELECT a, b FROM t ORDER BY a")
    }

    fn row(a: i64, b: &str) -> Vec<Value> {
        vec![Value::Int64(a), Value::String(b.to_string())]
    }

    #[test]
    fn should_delete_rows_filtered_by_exists_predicates() {
        // Arrange
        let fixture = dml_fixture("exists_delete");
        let statements = [
            "DELETE FROM t WHERE EXISTS (SELECT 1 FROM empty_t)",
            "DELETE FROM t WHERE a = 1 AND EXISTS (SELECT 1 FROM one_t)",
            "DELETE FROM t WHERE a = 2 AND NOT EXISTS (SELECT 1 FROM empty_t)",
        ];

        // Act
        let succeeded: Vec<bool> = statements
            .iter()
            .map(|sql| fixture.execute(sql).is_ok())
            .collect();

        // Assert
        assert_eq!(succeeded, vec![true; statements.len()]);
        assert_eq!(remaining(&fixture), vec![row(3, "r")]);
    }

    #[test]
    fn should_update_rows_filtered_by_exists_predicates() {
        // Arrange
        let fixture = dml_fixture("exists_update");
        let statements = [
            "UPDATE t SET b = 'x' WHERE EXISTS (SELECT 1 FROM empty_t)",
            "UPDATE t SET b = 'y' WHERE a = 1 AND EXISTS (SELECT 1 FROM one_t)",
            "UPDATE t SET b = 'z' WHERE a = 2 AND NOT EXISTS (SELECT 1 FROM empty_t)",
        ];

        // Act
        let succeeded: Vec<bool> = statements
            .iter()
            .map(|sql| fixture.execute(sql).is_ok())
            .collect();

        // Assert
        assert_eq!(succeeded, vec![true; statements.len()]);
        assert_eq!(
            remaining(&fixture),
            vec![row(1, "y"), row(2, "z"), row(3, "r")]
        );
    }

    #[test]
    fn should_apply_on_conflict_updates_guarded_by_exists() {
        // Arrange
        let fixture = dml_fixture("exists_on_conflict");
        let setup = fixture.execute("CREATE TABLE k (id INT PRIMARY KEY, v TEXT)");
        assert!(setup.is_ok(), "create k");
        assert!(
            fixture
                .execute("INSERT INTO k (id, v) VALUES (1, 'a')")
                .is_ok(),
            "seed k"
        );

        // Act
        let guarded = fixture.execute(
            "INSERT INTO k (id, v) VALUES (1, 'b') ON CONFLICT (id) DO UPDATE SET v = 'b' \
             WHERE EXISTS (SELECT 1 FROM one_t)",
        );

        // Assert
        assert!(guarded.is_ok(), "ON CONFLICT ... WHERE EXISTS should run");
        assert_eq!(
            fixture.rows("SELECT v FROM k"),
            vec![vec![Value::String("b".to_string())]]
        );
    }
}

mod exists_outside_where {
    use cassie::types::Value;

    use crate::support_sql_fixture::{sql_fixture, SqlFixture};

    fn clause_fixture(label: &str) -> SqlFixture {
        sql_fixture(
            label,
            &[
                "CREATE TABLE t (a INT, g TEXT)",
                "CREATE TABLE u (b INT)",
                "CREATE TABLE e (z INT)",
                "INSERT INTO t (a, g) VALUES (1, 'x')",
                "INSERT INTO t (a, g) VALUES (2, 'x')",
                "INSERT INTO u (b) VALUES (1)",
            ],
        )
    }

    #[test]
    fn should_evaluate_exists_wherever_a_boolean_is_legal() {
        // Arrange
        let fixture = clause_fixture("exists_clauses");

        // Act
        let having = fixture.rows(
            "SELECT g, COUNT(*) AS c FROM t GROUP BY g \
             HAVING COUNT(*) > 0 AND EXISTS (SELECT 1 FROM u)",
        );
        let having_empty = fixture
            .rows("SELECT g, COUNT(*) AS c FROM t GROUP BY g HAVING EXISTS (SELECT 1 FROM e)");
        let join_on =
            fixture.rows("SELECT t.a FROM t JOIN u ON t.a = u.b AND EXISTS (SELECT 1 FROM t)");
        let projection = fixture.rows(
            "SELECT a, EXISTS (SELECT 1 FROM e) AS e_any, NOT EXISTS (SELECT 1 FROM e) AS e_none \
             FROM t ORDER BY a",
        );
        let function_arg = fixture
            .rows("SELECT a FROM t WHERE COALESCE(NOT EXISTS (SELECT 1 FROM e), false) ORDER BY a");

        // Assert
        assert_eq!(
            having,
            vec![vec![Value::String("x".to_string()), Value::Int64(2)]]
        );
        assert!(
            having_empty.is_empty(),
            "HAVING EXISTS over an empty table keeps no group"
        );
        assert_eq!(join_on, vec![vec![Value::Int64(1)]]);
        assert_eq!(
            projection,
            vec![
                vec![Value::Int64(1), Value::Bool(false), Value::Bool(true)],
                vec![Value::Int64(2), Value::Bool(false), Value::Bool(true)],
            ]
        );
        assert_eq!(
            function_arg,
            vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
        );
    }
}

mod exists_cte_scope {
    use crate::support_sql_fixture::sql_fixture;

    #[test]
    fn should_bind_exists_subquery_relations_to_enclosing_ctes() {
        // Arrange
        let fixture = sql_fixture(
            "exists_cte_scope",
            &[
                "CREATE TABLE o (k INT)",
                "CREATE TABLE t (a INT)",
                "CREATE TABLE t2 (a INT)",
                "INSERT INTO o (k) VALUES (1)",
                "INSERT INTO t (a) VALUES (5)",
            ],
        );

        // Act
        let shadowing_empty = fixture.rows(
            "WITH t AS (SELECT a FROM t WHERE a = 999) \
             SELECT k FROM o WHERE EXISTS (SELECT 1 FROM t)",
        );
        let shadowing_full = fixture.rows(
            "WITH t2 AS (SELECT k AS a FROM o) SELECT k FROM o WHERE EXISTS (SELECT 1 FROM t2)",
        );
        let cte_only = fixture
            .rows("WITH c AS (SELECT k FROM o) SELECT k FROM o WHERE NOT EXISTS (SELECT 1 FROM c)");

        // Assert
        assert_eq!(
            (shadowing_empty.len(), shadowing_full.len(), cte_only.len()),
            (0, 1, 0)
        );
    }
}
