use cassie::app::{Cassie, CassieSession};
use cassie::config::{CassieRuntimeConfig, OperatorSwitchingEnabled};
use cassie::types::{Value, Vector};

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, with_fallback};

fn with_cassie(name: &str, test: impl FnOnce(&Cassie, &CassieSession)) {
    with_fallback();
    let path = data_dir(name);
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    let session = cassie.create_session("tester", None);
    test(&cassie, &session);
    let _ = std::fs::remove_dir_all(path);
}

fn with_bounded_join_cassie(name: &str, test: impl FnOnce(&Cassie, &CassieSession)) {
    with_fallback();
    let path = data_dir(name);
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.vectorized_joins_enabled = true;
    config.limits.adaptive_execution_enabled = false;
    config.limits.operator_switching_enabled = OperatorSwitchingEnabled::disabled();
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
    let session = cassie.create_session("tester", None);
    test(&cassie, &session);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_preserve_exact_large_bigint_ordering_across_relational_paths() {
    // Arrange
    with_cassie("semantic_large_bigints", |cassie, session| {
        cassie
            .execute_sql(
                session,
                "CREATE TABLE semantic_large_bigints (value BIGINT)",
                vec![],
            )
            .expect("create table");
        for value in [9_007_199_254_740_993_i64, 9_007_199_254_740_992_i64] {
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO semantic_large_bigints (value) VALUES ($1)",
                    vec![Value::Int64(value)],
                )
                .expect("insert value");
        }

        // Act
        let result = cassie
            .execute_sql(
                session,
                "SELECT value, row_number() OVER (ORDER BY value) AS ordinal FROM semantic_large_bigints ORDER BY value",
                vec![],
            )
            .expect("query");

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::Int64(9_007_199_254_740_992), Value::Int64(1)],
                vec![Value::Int64(9_007_199_254_740_993), Value::Int64(2)],
            ]
        );
    });
}

#[test]
fn should_compute_min_max_with_negative_mixed_numeric_inputs() {
    // Arrange
    with_cassie("semantic_numeric_minmax", |cassie, session| {
        cassie
            .execute_sql(
                session,
                "CREATE TABLE semantic_negative_numbers (value BIGINT)",
                vec![],
            )
            .expect("create negative table");
        for value in [-10_i64, -2, -30] {
            cassie
                .execute_sql(
                    session,
                    &format!("INSERT INTO semantic_negative_numbers (value) VALUES ({value})"),
                    vec![],
                )
                .expect("insert negative value");
        }
        cassie
            .execute_sql(
                session,
                "CREATE TABLE semantic_integer_number (value BIGINT)",
                vec![],
            )
            .expect("create integer table");
        cassie
            .execute_sql(
                session,
                "CREATE TABLE semantic_float_number (value FLOAT)",
                vec![],
            )
            .expect("create float table");
        cassie
            .execute_sql(
                session,
                "INSERT INTO semantic_integer_number (value) VALUES (10)",
                vec![],
            )
            .expect("insert integer");
        cassie
            .execute_sql(
                session,
                "INSERT INTO semantic_float_number (value) VALUES (-2.5)",
                vec![],
            )
            .expect("insert float");

        // Act
        let negative = cassie
            .execute_sql(
                session,
                "SELECT min(value), max(value) FROM semantic_negative_numbers",
                vec![],
            )
            .expect("negative aggregate");
        let mixed = cassie
            .execute_sql(
                session,
                "SELECT min(value), max(value) FROM (SELECT value FROM semantic_integer_number UNION ALL SELECT value FROM semantic_float_number) AS mixed_values",
                vec![],
            )
            .expect("mixed aggregate");

        // Assert
        assert_eq!(
            negative.rows,
            vec![vec![Value::Int64(-30), Value::Int64(-2)]]
        );
        assert_eq!(
            mixed.rows,
            vec![vec![Value::Float64(-2.5), Value::Int64(10)]]
        );
    });
}

#[test]
fn should_propagate_order_by_expression_errors() {
    // Arrange
    with_cassie("semantic_sort_errors", |cassie, session| {
        cassie
            .execute_sql(
                session,
                "CREATE TABLE semantic_sort_errors (label TEXT, divisor INT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                session,
                "INSERT INTO semantic_sort_errors (label, divisor) VALUES ('invalid', 0)",
                vec![],
            )
            .expect("insert row");

        // Act
        let error = cassie
            .execute_sql(
                session,
                "SELECT label FROM semantic_sort_errors ORDER BY 1 / divisor",
                vec![],
            )
            .expect_err("division by zero must remain an ORDER BY error");

        // Assert
        assert!(error.to_string().contains("division by zero"));
    });
}

#[test]
fn should_apply_postgres_null_ordering_across_relational_sorts() {
    // Arrange
    with_cassie("semantic_null_ordering", |cassie, session| {
        cassie
            .execute_sql(
                session,
                "CREATE TABLE semantic_null_ordering (label TEXT, value INT)",
                vec![],
            )
            .expect("create table");
        for sql in [
            "INSERT INTO semantic_null_ordering (label, value) VALUES ('low', 1)",
            "INSERT INTO semantic_null_ordering (label, value) VALUES ('high', 2)",
            "INSERT INTO semantic_null_ordering (label, value) VALUES ('missing', NULL)",
        ] {
            cassie
                .execute_sql(session, sql, vec![])
                .expect("insert row");
        }

        // Act
        let ascending = cassie
            .execute_sql(
                session,
                "SELECT label FROM semantic_null_ordering ORDER BY value ASC LIMIT 3",
                vec![],
            )
            .expect("ascending query");
        let descending = cassie
            .execute_sql(
                session,
                "SELECT label FROM semantic_null_ordering ORDER BY value DESC LIMIT 3",
                vec![],
            )
            .expect("descending query");
        let nulls_first = cassie
            .execute_sql(
                session,
                "SELECT label, row_number() OVER (ORDER BY value DESC NULLS FIRST) AS ordinal FROM semantic_null_ordering ORDER BY label",
                vec![],
            )
            .expect("window nulls first query");
        let nulls_last = cassie
            .execute_sql(
                session,
                "SELECT label, row_number() OVER (ORDER BY value DESC NULLS LAST) AS ordinal FROM semantic_null_ordering ORDER BY label",
                vec![],
            )
            .expect("window nulls last query");

        // Assert
        assert_eq!(
            ascending.rows,
            vec![
                vec![Value::String("low".into())],
                vec![Value::String("high".into())],
                vec![Value::String("missing".into())],
            ]
        );
        assert_eq!(
            descending.rows,
            vec![
                vec![Value::String("missing".into())],
                vec![Value::String("high".into())],
                vec![Value::String("low".into())],
            ]
        );
        assert_eq!(
            nulls_first.rows,
            vec![
                vec![Value::String("high".into()), Value::Int64(2)],
                vec![Value::String("low".into()), Value::Int64(3)],
                vec![Value::String("missing".into()), Value::Int64(1)],
            ]
        );
        assert_eq!(
            nulls_last.rows,
            vec![
                vec![Value::String("high".into()), Value::Int64(1)],
                vec![Value::String("low".into()), Value::Int64(2)],
                vec![Value::String("missing".into()), Value::Int64(3)],
            ]
        );
    });
}

#[test]
fn should_preserve_positional_set_semantics_with_left_names() {
    // Arrange
    with_cassie("semantic_set_aliases", |cassie, session| {
        cassie
            .execute_sql(
                session,
                "CREATE TABLE semantic_set_left (value TEXT)",
                vec![],
            )
            .expect("create left");
        cassie
            .execute_sql(
                session,
                "CREATE TABLE semantic_set_right (value TEXT)",
                vec![],
            )
            .expect("create right");
        for table in ["semantic_set_left", "semantic_set_right"] {
            cassie
                .execute_sql(
                    session,
                    &format!("INSERT INTO {table} (value) VALUES ('shared')"),
                    vec![],
                )
                .expect("insert set row");
        }

        // Act
        let result = cassie
            .execute_sql(
                session,
                "SELECT value AS left_name FROM semantic_set_left INTERSECT SELECT value AS right_name FROM semantic_set_right",
                vec![],
            )
            .expect("set query");

        // Assert
        assert_eq!(result.columns[0].name, "left_name");
        assert_eq!(result.rows, vec![vec![Value::String("shared".into())]]);
    });
}

#[test]
fn should_use_left_set_names_when_the_left_operand_is_empty() {
    // Arrange
    with_cassie("semantic_empty_left_set", |cassie, session| {
        cassie
            .execute_sql(
                session,
                "CREATE TABLE semantic_empty_set_left (value TEXT)",
                vec![],
            )
            .expect("create left table");
        cassie
            .execute_sql(
                session,
                "CREATE TABLE semantic_empty_set_right (value TEXT)",
                vec![],
            )
            .expect("create right table");
        for value in ["a", "z"] {
            cassie
                .execute_sql(
                    session,
                    &format!("INSERT INTO semantic_empty_set_right (value) VALUES ('{value}')"),
                    vec![],
                )
                .expect("insert right row");
        }

        // Act
        let result = cassie
            .execute_sql(
                session,
                "SELECT value AS left_name FROM semantic_empty_set_left UNION ALL SELECT value AS right_name FROM semantic_empty_set_right ORDER BY left_name DESC",
                vec![],
            )
            .expect("set query");

        // Assert
        assert_eq!(result.columns[0].name, "left_name");
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("z".into())],
                vec![Value::String("a".into())],
            ]
        );
    });
}

#[test]
fn should_keep_composite_relational_keys_collision_free() {
    // Arrange
    with_cassie("semantic_composite_keys", |cassie, session| {
        cassie
            .execute_sql(
                session,
                "CREATE TABLE semantic_composite_keys (bucket TEXT, first_part TEXT, second_part TEXT)",
                vec![],
            )
            .expect("create table");
        for sql in [
            "INSERT INTO semantic_composite_keys (bucket, first_part, second_part) VALUES ('one', 'a|4:b', 'c')",
            "INSERT INTO semantic_composite_keys (bucket, first_part, second_part) VALUES ('one', 'a', 'b|4:c')",
        ] {
            cassie.execute_sql(session, sql, vec![]).expect("insert row");
        }

        // Act
        let grouped = cassie
            .execute_sql(
                session,
                "SELECT first_part, second_part, count(*) AS total FROM semantic_composite_keys GROUP BY first_part, second_part ORDER BY first_part, second_part",
                vec![],
            )
            .expect("group query");
        let distinct = cassie
            .execute_sql(
                session,
                "SELECT DISTINCT first_part, second_part FROM semantic_composite_keys ORDER BY first_part, second_part",
                vec![],
            )
            .expect("distinct query");
        let distinct_on = cassie
            .execute_sql(
                session,
                "SELECT DISTINCT ON (first_part, second_part) first_part, second_part FROM semantic_composite_keys ORDER BY first_part, second_part",
                vec![],
            )
            .expect("distinct on query");
        let windowed = cassie
            .execute_sql(
                session,
                "SELECT first_part, second_part, row_number() OVER (PARTITION BY first_part, second_part ORDER BY first_part) AS row_number, rank() OVER (PARTITION BY bucket ORDER BY first_part, second_part) AS rank FROM semantic_composite_keys ORDER BY first_part, second_part",
                vec![],
            )
            .expect("window query");

        // Assert
        assert_eq!(grouped.rows.len(), 2);
        assert!(grouped.rows.iter().all(|row| row[2] == Value::Int64(1)));
        assert_eq!(distinct.rows.len(), 2);
        assert_eq!(distinct_on.rows.len(), 2);
        assert_eq!(windowed.rows.len(), 2);
        assert!(windowed.rows.iter().all(|row| row[2] == Value::Int64(1)));
        assert_eq!(
            windowed
                .rows
                .iter()
                .map(|row| row[3].clone())
                .collect::<Vec<_>>(),
            vec![Value::Int64(1), Value::Int64(2)]
        );
    });
}

#[test]
fn should_match_integer_float_joins_with_scalar_numeric_equality() {
    // Arrange
    with_cassie("semantic_numeric_joins", |cassie, session| {
        cassie
            .execute_sql(
                session,
                "CREATE TABLE semantic_join_ints (join_key BIGINT, label TEXT)",
                vec![],
            )
            .expect("create ints");
        cassie
            .execute_sql(
                session,
                "CREATE TABLE semantic_join_floats (join_key FLOAT, label TEXT)",
                vec![],
            )
            .expect("create floats");
        for sql in [
            "INSERT INTO semantic_join_ints (join_key, label) VALUES (1, 'small')",
            "INSERT INTO semantic_join_floats (join_key, label) VALUES (1.0, 'small')",
            "INSERT INTO semantic_join_floats (join_key, label) VALUES (9007199254740992.0, 'rounded')",
        ] {
            cassie.execute_sql(session, sql, vec![]).expect("insert row");
        }
        cassie
            .execute_sql(
                session,
                "INSERT INTO semantic_join_ints (join_key, label) VALUES ($1, 'large')",
                vec![Value::Int64(9_007_199_254_740_993)],
            )
            .expect("insert exact large integer");

        // Act
        let keyed = cassie
            .execute_sql(
                session,
                "SELECT semantic_join_ints.label FROM semantic_join_ints JOIN semantic_join_floats ON semantic_join_ints.join_key = semantic_join_floats.join_key ORDER BY semantic_join_ints.label",
                vec![],
            )
            .expect("keyed join");
        let scalar = cassie
            .execute_sql(
                session,
                "SELECT semantic_join_ints.label FROM semantic_join_ints JOIN semantic_join_floats ON true WHERE semantic_join_ints.join_key = semantic_join_floats.join_key ORDER BY semantic_join_ints.label",
                vec![],
            )
            .expect("scalar join");

        // Assert
        let expected = vec![vec![Value::String("small".into())]];
        assert_eq!(keyed.rows, expected);
        assert_eq!(scalar.rows, expected);
    });
}

#[test]
fn should_preserve_numeric_equality_when_a_bounded_join_side_is_indexed() {
    // Arrange
    with_bounded_join_cassie("semantic_indexed_numeric_join", |cassie, session| {
        cassie
            .execute_sql(
                session,
                "CREATE TABLE semantic_indexed_join_ints (join_key BIGINT, label TEXT)",
                vec![],
            )
            .expect("create integer table");
        cassie
            .execute_sql(
                session,
                "CREATE TABLE semantic_indexed_join_floats (join_key FLOAT, label TEXT)",
                vec![],
            )
            .expect("create float table");
        cassie
            .execute_sql(
                session,
                "INSERT INTO semantic_indexed_join_ints (join_key, label) VALUES (1, 'matched')",
                vec![],
            )
            .expect("insert integer row");
        cassie
            .execute_sql(
                session,
                "INSERT INTO semantic_indexed_join_floats (join_key, label) VALUES (1.0, 'matched')",
                vec![],
            )
            .expect("insert float row");
        cassie
            .execute_sql(
                session,
                "CREATE INDEX semantic_indexed_join_floats_key_idx ON semantic_indexed_join_floats USING btree (join_key)",
                vec![],
            )
            .expect("create float index");

        // Act
        let result = cassie
            .execute_sql(
                session,
                "SELECT semantic_indexed_join_ints.label FROM semantic_indexed_join_ints JOIN semantic_indexed_join_floats ON semantic_indexed_join_ints.join_key = semantic_indexed_join_floats.join_key LIMIT 10",
                vec![],
            )
            .expect("bounded indexed join");

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("matched".into())]]);
    });
}

#[test]
fn should_evaluate_same_dimension_vector_parameters_independently() {
    // Arrange
    with_cassie("semantic_vector_parameters", |cassie, session| {
        let sql = "SELECT vector_distance($1, '[1,0]')";

        // Act
        let first = cassie
            .execute_sql(
                session,
                sql,
                vec![Value::Vector(Vector::new(vec![1.0, 0.0]))],
            )
            .expect("first vector query");
        let second = cassie
            .execute_sql(
                session,
                sql,
                vec![Value::Vector(Vector::new(vec![0.0, 1.0]))],
            )
            .expect("second vector query");

        // Assert
        assert_eq!(first.rows, vec![vec![Value::Float64(0.0)]]);
        let Value::Float64(distance) = second.rows[0][0] else {
            panic!("expected floating vector distance");
        };
        assert!((distance - 2.0_f64.sqrt()).abs() < 1e-12);
    });
}

#[test]
fn should_invalidate_immutable_udf_results_on_recreation() {
    // Arrange
    with_cassie("semantic_udf_recreate", |cassie, session| {
        cassie
            .execute_sql(
                session,
                r#"CREATE FUNCTION semantic_recreated(x INT) RETURNS INT IMMUTABLE AS "x""#,
                vec![],
            )
            .expect("create original function");
        let original = cassie
            .execute_sql(session, "SELECT semantic_recreated(1)", vec![])
            .expect("execute original");
        cassie
            .execute_sql(session, "DROP FUNCTION semantic_recreated", vec![])
            .expect("drop function");
        cassie
            .execute_sql(
                session,
                r#"CREATE FUNCTION semantic_recreated(x INT) RETURNS INT IMMUTABLE AS "x + 1""#,
                vec![],
            )
            .expect("recreate function");

        // Act
        let recreated = cassie
            .execute_sql(session, "SELECT semantic_recreated(1)", vec![])
            .expect("execute recreated");

        // Assert
        assert_eq!(original.rows, vec![vec![Value::Int64(1)]]);
        assert_eq!(recreated.rows, vec![vec![Value::Int64(2)]]);
    });
}
