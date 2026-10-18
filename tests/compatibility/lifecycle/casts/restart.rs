use super::*;
use cassie::types::Value;

#[test]
fn should_preserve_cast_dependencies_given_persisted_cast_objects_when_storage_restarts() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("sql_cast_dependency_restart");
    let _directory = DataDirectory(path.clone());
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie instance");
    cassie.startup().expect("start Cassie instance");
    let session = cassie.create_session("tester", None);
    for statement in [
        "CREATE TYPE compat_cast_restart_source AS (value INTEGER)",
        "CREATE TYPE compat_cast_restart_target AS (value BIGINT)",
        "CREATE FUNCTION compat_cast_restart_fn(compat_cast_restart_source) \
         RETURNS compat_cast_restart_target LANGUAGE SQL IMMUTABLE STRICT AS \
         $$ SELECT ROW(($1).value::BIGINT)::compat_cast_restart_target $$",
        "CREATE CAST (compat_cast_restart_source AS compat_cast_restart_target) \
         WITH FUNCTION compat_cast_restart_fn(compat_cast_restart_source) AS ASSIGNMENT",
    ] {
        cassie
            .execute_sql(&session, statement, vec![])
            .expect("persist the composite cast and its function dependency");
    }
    drop(session);
    drop(cassie);

    // Act
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie instance");
    restarted.startup().expect("recover cast catalog objects");
    let session = restarted.create_session("tester", None);
    let value = restarted
        .execute_sql(
            &session,
            "SELECT ((ROW(7)::compat_cast_restart_source)::compat_cast_restart_target).value",
            vec![],
        )
        .expect("resolve the persisted cast and function after restart")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(value, vec![vec![Value::Int64(7)]]);
}
