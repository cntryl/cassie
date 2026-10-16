use super::*;
use cassie::types::Value;

#[test]
fn should_preserve_persisted_operator_behavior_given_persisted_operator_objects_when_storage_restarts(
) {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("operator_family_index_restart");
    let _directory = DataDirectory(path.clone());
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie instance");
    cassie.startup().expect("start Cassie instance");
    let session = cassie.create_session("tester", None);
    for statement in [
        "CREATE FUNCTION compat_restart_same(integer, integer) \
         RETURNS boolean LANGUAGE SQL IMMUTABLE STRICT AS 'SELECT $1 = $2'",
        "CREATE OPERATOR ## (LEFTARG = integer, RIGHTARG = integer, \
         PROCEDURE = compat_restart_same)",
        "CREATE OPERATOR FAMILY compat_restart_family USING btree",
        "CREATE OPERATOR CLASS compat_restart_ops FOR TYPE integer USING btree \
         FAMILY compat_restart_family AS OPERATOR 3 ##, \
         FUNCTION 1 pg_catalog.btint4cmp(integer, integer)",
        "CREATE TABLE compat_restart_operator_rows (value integer)",
        "INSERT INTO compat_restart_operator_rows VALUES (1), (2), (2), (3)",
        "CREATE INDEX compat_restart_operator_idx ON compat_restart_operator_rows \
         USING btree (value compat_restart_ops)",
    ] {
        cassie
            .execute_sql(&session, statement, vec![])
            .expect("create a persisted operator, family, class, and index");
    }
    drop(session);
    drop(cassie);

    // Act
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie instance");
    restarted
        .startup()
        .expect("recover operator and index catalog objects");
    let session = restarted.create_session("tester", None);
    let rows = restarted
        .execute_sql(
            &session,
            "SELECT value FROM compat_restart_operator_rows \
             WHERE value ## 2 ORDER BY value",
            vec![],
        )
        .expect("resolve the custom operator and use its persisted index after restart")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(2)], vec![Value::Int64(2)]]);
}
