use super::*;
use cassie::types::Value;

#[test]
fn should_preserve_inheritance_scan_behavior_given_persisted_parent_and_child_tables_when_storage_restarts(
) {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("inheritance_scan_restart");
    let _directory = DataDirectory(path.clone());
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie instance");
    cassie.startup().expect("start Cassie instance");
    let session = cassie.create_session("tester", None);
    for statement in [
        "CREATE TABLE compat_restart_inherit_parent (id INT, label TEXT)",
        "CREATE TABLE compat_restart_inherit_child (child_note TEXT) \
         INHERITS (compat_restart_inherit_parent)",
        "INSERT INTO compat_restart_inherit_parent VALUES (1, 'parent')",
        "INSERT INTO compat_restart_inherit_child VALUES (2, 'child', 'note')",
    ] {
        cassie
            .execute_sql(&session, statement, vec![])
            .expect("create and seed the inheritance hierarchy");
    }
    drop(session);
    drop(cassie);

    // Act
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie instance");
    restarted
        .startup()
        .expect("recover the inheritance catalog");
    let session = restarted.create_session("tester", None);
    let rows = restarted
        .execute_sql(
            &session,
            "SELECT id FROM compat_restart_inherit_parent ORDER BY id",
            vec![],
        )
        .expect("scan the parent and inherited child after restart")
        .rows()
        .to_vec();
    let edges = restarted
        .execute_sql(
            &session,
            "SELECT count(*)::BIGINT FROM pg_catalog.pg_inherits AS inheritance \
             JOIN pg_catalog.pg_class AS parent ON parent.oid = inheritance.inhparent \
             JOIN pg_catalog.pg_class AS child ON child.oid = inheritance.inhrelid \
             WHERE parent.relname = 'compat_restart_inherit_parent' \
               AND child.relname = 'compat_restart_inherit_child'",
            vec![],
        )
        .expect("read the persisted parent-child catalog edge")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]);
    assert_eq!(edges, vec![vec![Value::Int64(1)]]);
}
