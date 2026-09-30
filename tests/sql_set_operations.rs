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

mod set_operation_row_caps {
    use cassie::app::{Cassie, CassieSession};
    use cassie::config::CassieRuntimeConfig;
    use cassie::types::Value;

    fn capped_instance(label: &str) -> (Cassie, CassieSession, String) {
        crate::support_sql::use_local_storage();
        let path = crate::support_sql::data_dir(label);
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.max_result_rows = 5;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        let mut setup = vec![
            "CREATE TABLE smallt (n INT)".to_string(),
            "CREATE TABLE biggt (n INT)".to_string(),
            "INSERT INTO smallt (n) VALUES (0)".to_string(),
            "INSERT INTO smallt (n) VALUES (19)".to_string(),
        ];
        setup.extend((0..20).map(|n| format!("INSERT INTO biggt (n) VALUES ({n})")));
        for sql in &setup {
            assert!(
                cassie.execute_sql(&session, sql, vec![]).is_ok(),
                "setup failed: {sql}"
            );
        }
        (cassie, session, path)
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
    fn should_materialize_the_whole_right_branch_under_a_result_row_cap() {
        // Arrange
        let (cassie, session, path) = capped_instance("set_right_cap");
        let run = |sql: &str| {
            let Ok(result) = cassie.execute_sql(&session, sql, vec![]) else {
                panic!("query failed: {sql}");
            };
            sorted_ints(result.rows)
        };

        // Act
        let except = run("SELECT n FROM smallt EXCEPT SELECT n FROM biggt");
        let intersect = run("SELECT n FROM smallt INTERSECT SELECT n FROM biggt");
        let filtered = run("SELECT n FROM smallt INTERSECT SELECT n FROM biggt WHERE n >= 0");
        let big_left = run("SELECT n FROM biggt INTERSECT SELECT n FROM smallt");
        let cte_right =
            run("WITH b AS (SELECT n FROM biggt) SELECT n FROM smallt EXCEPT SELECT n FROM b");
        let derived_right =
            run("SELECT n FROM smallt EXCEPT SELECT n FROM (SELECT n FROM biggt) AS d");
        let recursive_right = run(
            "WITH RECURSIVE r(n) AS (SELECT 0 UNION ALL SELECT n + 1 FROM r WHERE n < 19) \
             SELECT n FROM smallt EXCEPT SELECT n FROM r",
        );

        // Assert
        assert_eq!(except, Vec::<i64>::new());
        assert_eq!(intersect, vec![0, 19]);
        assert_eq!(filtered, vec![0, 19]);
        assert_eq!(big_left, vec![0, 19]);
        assert_eq!(cte_right, Vec::<i64>::new());
        assert_eq!(derived_right, Vec::<i64>::new());
        assert_eq!(recursive_right, Vec::<i64>::new());
        let _ = std::fs::remove_dir_all(path);
    }
}

mod set_operation_chains {
    use cassie::types::Value;

    use crate::support_sql_fixture::{sql_fixture, SqlFixture};

    fn chain_fixture(label: &str) -> SqlFixture {
        sql_fixture(
            label,
            &[
                "CREATE TABLE sa (n INT)",
                "CREATE TABLE sb (n INT)",
                "CREATE TABLE sc (n INT)",
                "CREATE TABLE sd (n INT)",
                "INSERT INTO sa (n) VALUES (1)",
                "INSERT INTO sa (n) VALUES (2)",
                "INSERT INTO sb (n) VALUES (1)",
                "INSERT INTO sc (n) VALUES (2)",
                "INSERT INTO sd (n) VALUES (3)",
            ],
        )
    }

    fn ints(rows: Vec<Vec<Value>>) -> Vec<i64> {
        rows.into_iter()
            .map(|row| match row.first() {
                Some(Value::Int64(value)) => *value,
                other => panic!("expected an integer cell, got {other:?}"),
            })
            .collect()
    }

    fn sorted(fixture: &SqlFixture, sql: &str) -> Vec<i64> {
        let mut values = ints(fixture.rows(sql));
        values.sort_unstable();
        values
    }

    #[test]
    fn should_evaluate_mixed_set_operators_left_to_right_with_intersect_first() {
        // Arrange
        let fixture = chain_fixture("set_chain_precedence");

        // Act
        let intersect_union = sorted(
            &fixture,
            "SELECT n FROM sa INTERSECT SELECT n FROM sb UNION SELECT n FROM sd",
        );
        let except_union = sorted(
            &fixture,
            "SELECT n FROM sa EXCEPT SELECT n FROM sb UNION SELECT n FROM sd",
        );
        let union_all_union = sorted(
            &fixture,
            "SELECT n FROM sa UNION ALL SELECT n FROM sb UNION SELECT n FROM sb",
        );
        let union_intersect = sorted(
            &fixture,
            "SELECT n FROM sa UNION SELECT n FROM sb INTERSECT SELECT n FROM sd",
        );
        let union_all_intersect = sorted(
            &fixture,
            "SELECT n FROM sa UNION ALL SELECT n FROM sb UNION ALL \
             SELECT n FROM sb INTERSECT SELECT n FROM sa",
        );

        // Assert
        assert_eq!(intersect_union, vec![1, 3]);
        assert_eq!(except_union, vec![2, 3]);
        assert_eq!(union_all_union, vec![1, 2]);
        assert_eq!(union_intersect, vec![1, 2]);
        assert_eq!(union_all_intersect, vec![1, 1, 1, 2]);
    }

    #[test]
    fn should_accept_intersect_or_except_after_the_first_set_operator() {
        // Arrange
        let fixture = chain_fixture("set_chain_second_operator");

        // Act
        let except_except = sorted(
            &fixture,
            "SELECT n FROM sa EXCEPT SELECT n FROM sb EXCEPT SELECT n FROM sc",
        );
        let intersect_intersect = sorted(
            &fixture,
            "SELECT n FROM sa INTERSECT SELECT n FROM sa INTERSECT SELECT n FROM sb",
        );
        let union_except = sorted(
            &fixture,
            "SELECT n FROM sa UNION SELECT n FROM sb EXCEPT SELECT n FROM sb",
        );

        // Assert
        assert_eq!(except_except, Vec::<i64>::new());
        assert_eq!(intersect_intersect, vec![1]);
        assert_eq!(union_except, vec![2]);
    }

    #[test]
    fn should_resolve_global_clauses_over_a_regrouped_set_chain() {
        // Arrange
        let fixture = chain_fixture("set_chain_global_clauses");

        // Act
        let ordered = ints(fixture.rows(
            "SELECT n FROM sa EXCEPT SELECT n FROM sb UNION SELECT n FROM sd \
             ORDER BY 1 DESC LIMIT 1",
        ));
        let aliased = fixture.execute(
            "SELECT n AS m FROM sa EXCEPT SELECT n FROM sb UNION SELECT n FROM sd ORDER BY m DESC",
        );
        let cte_right = sorted(
            &fixture,
            "WITH x AS (SELECT n FROM sd) \
             SELECT n FROM sa EXCEPT SELECT n FROM sb UNION SELECT n FROM x",
        );
        let cte_left = sorted(
            &fixture,
            "WITH x AS (SELECT n FROM sa) \
             SELECT n FROM x EXCEPT SELECT n FROM sb UNION SELECT n FROM sd",
        );

        let Ok(regrouped) =
            fixture.execute("SELECT n FROM sa EXCEPT SELECT n FROM sb UNION SELECT n FROM sd")
        else {
            panic!("regrouped chain failed");
        };
        let Ok(plain) = fixture.execute("SELECT n FROM sa UNION SELECT n FROM sd") else {
            panic!("plain union failed");
        };

        // Assert
        assert_eq!(regrouped.columns, plain.columns);
        assert_eq!(ordered, vec![3]);
        let Ok(aliased) = aliased else {
            panic!("aliased regrouped chain failed");
        };
        assert_eq!(aliased.columns[0].name, "m");
        assert_eq!(ints(aliased.rows), vec![3, 2]);
        assert_eq!(cte_right, vec![2, 3]);
        assert_eq!(cte_left, vec![2, 3]);
    }
}
