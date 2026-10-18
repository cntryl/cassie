use cassie::app::Cassie;
use cassie::types::Value;

use super::support_sql as support;
use support::*;

#[test]
fn should_reject_user_function_defined_only_in_another_database() {
    // Arrange
    let path = data_dir("udf_database_scope");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let postgres = cassie.create_session("tester", Some("postgres".to_string()));
        cassie
            .execute_sql(&postgres, "CREATE DATABASE tenant_a", vec![])
            .expect("create tenant_a");
        cassie
            .execute_sql(&postgres, "CREATE DATABASE tenant_b", vec![])
            .expect("create tenant_b");

        let tenant_a = cassie.create_session("tester", Some("tenant_a".to_string()));
        let tenant_b = cassie.create_session("tester", Some("tenant_b".to_string()));
        cassie
            .execute_sql(
                &tenant_a,
                "CREATE FUNCTION rate(x INT) RETURNS INT AS \"x * 1000\"",
                vec![],
            )
            .expect("create tenant_a function");

        // Act
        let result = cassie.execute_sql(&tenant_b, "SELECT rate(2)", vec![]);

        // Assert
        assert!(
            result.is_err(),
            "tenant_b must not resolve tenant_a's rate function"
        );

        let own_function = cassie
            .execute_sql(
                &tenant_b,
                "CREATE FUNCTION rate(x INT) RETURNS TEXT AS \"CAST(x AS TEXT)\"",
                vec![],
            )
            .expect("create tenant_b function");
        assert_eq!(
            own_function.rows(),
            [] as [std::vec::Vec<cassie::types::Value>; 0]
        );
        let result = cassie
            .execute_sql(&tenant_b, "SELECT rate(2)", vec![])
            .expect("execute tenant_b function");
        assert_eq!(result.columns()[0].data_type, "text");
        assert_eq!(result.rows(), vec![vec![Value::String("2".to_string())]]);

        let result = cassie
            .execute_sql(&tenant_a, "SELECT rate(2)", vec![])
            .expect("execute tenant_a function");
        assert_eq!(result.rows(), vec![vec![Value::Int64(2000)]]);
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_resolve_user_functions_by_ordered_search_path() {
    // Arrange
    let path = data_dir("udf_search_path");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", Some("postgres".to_string()));
        cassie
            .execute_sql(&session, "CREATE SCHEMA first_schema", vec![])
            .expect("create first schema");
        cassie
            .execute_sql(&session, "CREATE SCHEMA second_schema", vec![])
            .expect("create second schema");
        cassie
            .execute_sql(
                &session,
                "CREATE FUNCTION first_schema.pick(x INT) RETURNS INT AS \"x + 10\"",
                vec![],
            )
            .expect("create first-schema function");
        cassie
            .execute_sql(
                &session,
                "CREATE FUNCTION second_schema.pick(x INT) RETURNS INT AS \"x + 20\"",
                vec![],
            )
            .expect("create second-schema function");

        // Act
        cassie
            .execute_sql(
                &session,
                "SET search_path TO second_schema, first_schema",
                vec![],
            )
            .expect("set second schema first");
        let second_first = cassie
            .execute_sql(&session, "SELECT pick(2)", vec![])
            .expect("resolve second-schema function");
        cassie
            .execute_sql(
                &session,
                "SET search_path TO first_schema, second_schema",
                vec![],
            )
            .expect("set first schema first");
        let first_first = cassie
            .execute_sql(&session, "SELECT pick(2)", vec![])
            .expect("resolve first-schema function");

        // Assert
        assert_eq!(second_first.rows(), vec![vec![Value::Int64(22)]]);
        assert_eq!(first_first.rows(), vec![vec![Value::Int64(12)]]);
    });

    let _ = std::fs::remove_dir_all(path);
}
