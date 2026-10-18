use super::*;
use cassie::types::Value;

#[test]
fn should_preserve_aggregate_dependencies_given_persisted_sql_aggregates_when_storage_restarts() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("sql_aggregate_dependency_restart");
    let _directory = DataDirectory(path.clone());
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie instance");
    cassie.startup().expect("start Cassie instance");
    let session = cassie.create_session("tester", None);
    for statement in [
        "CREATE FUNCTION compat_aggregate_transition(INTEGER, INTEGER) \
         RETURNS INTEGER LANGUAGE SQL IMMUTABLE STRICT AS \
         'SELECT $1 + $2'",
        "CREATE AGGREGATE compat_persisted_sum(INTEGER) \
         (SFUNC = compat_aggregate_transition, STYPE = INTEGER, INITCOND = '0')",
        "CREATE TABLE compat_aggregate_restart_rows (value INTEGER NOT NULL)",
        "INSERT INTO compat_aggregate_restart_rows VALUES (2), (3), (5)",
    ] {
        cassie
            .execute_sql(&session, statement, vec![])
            .expect("create and seed the SQL aggregate dependency graph");
    }
    drop(session);
    drop(cassie);

    // Act
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie instance");
    restarted
        .startup()
        .expect("recover aggregate catalog objects");
    let session = restarted.create_session("tester", None);
    let result = restarted
        .execute_sql(
            &session,
            "SELECT compat_persisted_sum(value) FROM compat_aggregate_restart_rows",
            vec![],
        )
        .expect("resolve the aggregate and transition function after restart")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(result, vec![vec![Value::Int64(10)]]);
}
