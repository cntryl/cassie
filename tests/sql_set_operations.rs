// Integration suite: set operations (UNION, INTERSECT, EXCEPT) and the
// statements that embed them.

#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/sql_fixture.rs"]
mod support_sql_fixture;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

mod set_operation_views {
    use cassie::types::Value;

    use crate::support_sql_fixture::{sql_fixture, SqlFixture};

    fn set_fixture(label: &str) -> SqlFixture {
        sql_fixture(
            label,
            &[
                "CREATE TABLE sa (n INT)",
                "CREATE TABLE sb (n INT)",
                "INSERT INTO sa (n) VALUES (1)",
                "INSERT INTO sa (n) VALUES (2)",
                "INSERT INTO sb (n) VALUES (2)",
                "INSERT INTO sb (n) VALUES (5)",
            ],
        )
    }

    fn sorted_ints(rows: Vec<Vec<Value>>) -> Vec<i64> {
        let mut values: Vec<i64> = rows
            .into_iter()
            .map(|row| match row.first() {
                Some(Value::Int64(value)) => *value,
                other => panic!("expected an integer cell, got {other:?}"),
            })
            .collect();
        values.sort_unstable();
        values
    }

    #[test]
    fn should_keep_every_set_operation_branch_in_a_view_definition() {
        // Arrange
        let fixture = set_fixture("view_set_ops");
        for sql in [
            "CREATE VIEW v_union AS SELECT n FROM sa UNION ALL SELECT n FROM sb",
            "CREATE VIEW v_intersect AS SELECT n FROM sa INTERSECT SELECT n FROM sb",
            "CREATE VIEW v_except AS SELECT n FROM sa EXCEPT SELECT n FROM sb",
        ] {
            assert!(fixture.execute(sql).is_ok(), "view creation failed: {sql}");
        }

        // Act
        let union = sorted_ints(fixture.rows("SELECT n FROM v_union"));
        let intersect = sorted_ints(fixture.rows("SELECT n FROM v_intersect"));
        let except = sorted_ints(fixture.rows("SELECT n FROM v_except"));

        // Assert
        assert_eq!(union, vec![1, 2, 2, 5]);
        assert_eq!(intersect, vec![2]);
        assert_eq!(except, vec![1]);
    }

    #[test]
    fn should_keep_the_with_clause_in_a_view_definition() {
        // Arrange
        let fixture = set_fixture("view_with_clause");
        let created = fixture.execute(
            "CREATE VIEW v_cte AS WITH big AS (SELECT n FROM sb WHERE n > 2) SELECT n FROM big",
        );
        assert!(created.is_ok(), "view with a WITH clause should be created");

        // Act
        let rows = sorted_ints(fixture.rows("SELECT n FROM v_cte"));

        // Assert
        assert_eq!(rows, vec![5]);
    }
}
