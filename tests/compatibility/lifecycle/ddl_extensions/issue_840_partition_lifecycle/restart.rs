use super::*;

#[test]
fn should_restore_partition_lifecycle_after_restart() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("issue840_partition_restart");
    let _directory = DataDirectory(path.clone());
    let before_restart = {
        let cassie = Cassie::new_with_data_dir(&path).expect("create local Cassie instance");
        cassie.startup().expect("start local Cassie instance");
        let session = cassie.create_session("tester", None);
        for statement in [
            "CREATE SCHEMA compat_issue840_restart",
            "CREATE TABLE compat_issue840_restart.events (id INT, payload TEXT) \
             PARTITION BY RANGE (id)",
            "CREATE TABLE compat_issue840_restart.events_low \
             PARTITION OF compat_issue840_restart.events \
             FOR VALUES FROM (MINVALUE) TO (10)",
            "CREATE TABLE compat_issue840_restart.events_high \
             PARTITION OF compat_issue840_restart.events \
             FOR VALUES FROM (10) TO (MAXVALUE)",
            "INSERT INTO compat_issue840_restart.events VALUES (1, 'low'), (10, 'high')",
        ] {
            cassie
                .execute_sql(&session, statement, vec![])
                .unwrap_or_else(|error| panic!("execute before restart ({statement}): {error}"));
        }
        let catalog = cassie
            .execute_sql(
                &session,
                "SELECT parent.oid::BIGINT, child.oid::BIGINT, \
                        partitioning.partstrat::TEXT, partitioning.partnatts::BIGINT, \
                        child.relispartition \
                 FROM pg_catalog.pg_class AS parent \
                 JOIN pg_catalog.pg_partitioned_table AS partitioning \
                   ON partitioning.partrelid = parent.oid \
                 JOIN pg_catalog.pg_inherits AS inheritance \
                   ON inheritance.inhparent = parent.oid \
                 JOIN pg_catalog.pg_class AS child \
                   ON child.oid = inheritance.inhrelid \
                 WHERE parent.oid = 'compat_issue840_restart.events'::regclass \
                 ORDER BY child.relname",
                vec![],
            )
            .expect("read partition catalog identity before restart")
            .rows()
            .to_vec();
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT id::TEXT, payload FROM compat_issue840_restart.events ORDER BY id",
                vec![],
            )
            .expect("read parent rows before restart")
            .rows()
            .to_vec();
        drop(session);
        drop(cassie);
        (catalog, rows)
    };

    // Act
    let reopened = Cassie::new_with_data_dir(&path).expect("reopen local Cassie instance");
    reopened
        .startup()
        .expect("recover partition relations after restart");
    let session = reopened.create_session("tester", None);
    let catalog_after_restart = reopened
        .execute_sql(
            &session,
            "SELECT parent.oid::BIGINT, child.oid::BIGINT, \
                    partitioning.partstrat::TEXT, partitioning.partnatts::BIGINT, \
                    child.relispartition \
             FROM pg_catalog.pg_class AS parent \
             JOIN pg_catalog.pg_partitioned_table AS partitioning \
               ON partitioning.partrelid = parent.oid \
             JOIN pg_catalog.pg_inherits AS inheritance \
               ON inheritance.inhparent = parent.oid \
             JOIN pg_catalog.pg_class AS child \
               ON child.oid = inheritance.inhrelid \
             WHERE parent.oid = 'compat_issue840_restart.events'::regclass \
             ORDER BY child.relname",
            vec![],
        )
        .expect("read partition catalog identity after restart")
        .rows()
        .to_vec();
    let rows_after_restart = reopened
        .execute_sql(
            &session,
            "SELECT id::TEXT, payload FROM compat_issue840_restart.events ORDER BY id",
            vec![],
        )
        .expect("read parent rows after restart")
        .rows()
        .to_vec();
    reopened
        .execute_sql(
            &session,
            "INSERT INTO compat_issue840_restart.events VALUES (11, 'after-restart')",
            vec![],
        )
        .expect("route a new row after restart");
    let high_partition_count = reopened
        .execute_sql(
            &session,
            "SELECT count(*) FROM compat_issue840_restart.events_high",
            vec![],
        )
        .expect("read the high partition after post-restart routing")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(catalog_after_restart, before_restart.0);
    assert_eq!(catalog_after_restart.len(), 2);
    assert_eq!(
        catalog_after_restart[0][2],
        cassie::types::Value::String("r".to_owned())
    );
    assert_eq!(catalog_after_restart[0][3], cassie::types::Value::Int64(1));
    assert_eq!(
        catalog_after_restart[0][4],
        cassie::types::Value::Bool(true)
    );
    assert_eq!(rows_after_restart, before_restart.1);
    assert_eq!(
        rows_after_restart,
        vec![
            vec![
                cassie::types::Value::String("1".to_owned()),
                cassie::types::Value::String("low".to_owned()),
            ],
            vec![
                cassie::types::Value::String("10".to_owned()),
                cassie::types::Value::String("high".to_owned()),
            ],
        ]
    );
    assert_eq!(high_partition_count.len(), 1);
    assert_eq!(high_partition_count[0][0], cassie::types::Value::Int64(2));
}
