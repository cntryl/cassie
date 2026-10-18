use super::support_read_equivalence::{sql, with_fixture_config};
use cassie::types::Value;

#[test]
fn should_deny_long_positional_prefix_output_backing() {
    // Arrange
    with_fixture_config(
        "typed_prefix_budget",
        |config| config.limits.query_memory_budget_bytes = 16 * 1024,
        |cassie, session| {
            let prefix = "p".repeat(4096);
            let sql = format!("WITH c AS (SELECT CAST(NULL AS TEXT[]) AS n, CAST(2 AS BIGINT) AS tail) SELECT * FROM c AS a(\"{prefix}\")");
            // Act
            let result = cassie.execute_sql(session, &sql, vec![]);
            let bad = cassie.execute_sql(
                session,
                "WITH c AS (SELECT CAST(1 AS BIGINT) AS n) SELECT * FROM c AS a(x,y)",
                vec![],
            );
            // Assert
            assert!(matches!(
                result,
                Err(cassie::app::CassieError::ResourceLimit(_))
            ));
            let error = bad.expect_err("binder alias arity retains priority");
            assert!(error.to_string().contains("more names than source columns"));
        },
    );
}

#[test]
fn should_preserve_qualified_order_through_dynamic_pagination() {
    // Arrange
    with_fixture_config(
        "typed_alias_bounds",
        |_| {},
        |cassie, session| {
            sql(
                cassie,
                session,
                "CREATE TABLE integration_scores (score INT, payload INT)",
            );
            sql(
                cassie,
                session,
                "INSERT INTO integration_scores VALUES (1,90),(2,10),(3,50)",
            );
            let qualified =
                "SELECT p.payload AS score FROM integration_scores AS p ORDER BY p.score";
            // Act
            let all = sql(cassie, session, qualified);
            let page = cassie
                .execute_sql(
                    session,
                    &format!("{qualified} LIMIT $1 OFFSET $2"),
                    vec![Value::Int64(1), Value::Int64(1)],
                )
                .expect("dynamic typed top k");
            let prefix = cassie
                .execute_sql(
                    session,
                    &format!("{qualified} LIMIT $1 OFFSET $2"),
                    vec![Value::Int64(2), Value::Int64(0)],
                )
                .expect("dynamic prefix");
            let unlimited = cassie
                .execute_sql(
                    session,
                    &format!("{qualified} LIMIT $1 OFFSET $2"),
                    vec![Value::Null, Value::Null],
                )
                .expect("NULL pagination");
            let zero = cassie
                .execute_sql(
                    session,
                    &format!("{qualified} LIMIT $1"),
                    vec![Value::Int64(0)],
                )
                .expect("zero pagination");
            let unqualified = sql(
                cassie,
                session,
                "SELECT p.payload AS score FROM integration_scores AS p ORDER BY score",
            );
            let quoted = sql(
                cassie,
                session,
                "SELECT p.payload AS \"score\" FROM integration_scores AS p ORDER BY p.score",
            );
            let hidden = cassie.execute_sql(
                session,
                "SELECT p.payload FROM integration_scores AS p ORDER BY integration_scores.score",
                vec![],
            );
            // Assert
            assert_eq!(
                all.rows(),
                vec![
                    vec![Value::Int64(90)],
                    vec![Value::Int64(10)],
                    vec![Value::Int64(50)]
                ]
            );
            assert_eq!(page.rows(), &all.rows()[1..2]);
            assert_eq!(prefix.rows(), &all.rows()[..2]);
            assert_eq!(unlimited.rows(), all.rows());
            assert_eq!(zero.rows(), [] as [Vec<Value>; 0]);
            assert_eq!(
                unqualified.rows(),
                vec![
                    vec![Value::Int64(10)],
                    vec![Value::Int64(50)],
                    vec![Value::Int64(90)]
                ]
            );
            assert_eq!(quoted.rows(), all.rows());
            assert_eq!(page.columns(), all.columns());
            assert_eq!(prefix.columns(), all.columns());
            assert!(hidden.is_err(), "alias hides original input qualifier");
        },
    );
}
