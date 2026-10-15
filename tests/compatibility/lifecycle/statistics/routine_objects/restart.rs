#[test]
fn should_collect_extended_statistics_restore_them_after_restart() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("extended-statistics-restart");
    let _directory = DataDirectory(path.clone());
    let expected = vec![vec![
        cassie::types::Value::String("compat_extended_stats".into()),
        cassie::types::Value::Bool(true),
        cassie::types::Value::Bool(true),
        cassie::types::Value::Bool(true),
    ]];
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie instance");
        cassie.startup().expect("start Cassie instance");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE compat_stats_rows (group_id INT, flag INT)",
            "INSERT INTO compat_stats_rows VALUES \
             (1,10),(1,10),(1,10),(1,10),(2,20),(2,20),(2,20),(2,20), \
             (3,30),(3,30),(3,30),(3,30),(4,40),(4,40),(4,40),(4,40), \
             (5,50),(5,50),(5,50),(5,50)",
            "CREATE STATISTICS compat_extended_stats (ndistinct, dependencies, mcv) \
             ON group_id, flag FROM compat_stats_rows",
            "ANALYZE compat_stats_rows",
        ] {
            cassie
                .execute_sql(&session, sql, vec![])
                .unwrap_or_else(|error| panic!("could not execute {sql}: {error}"));
        }
        drop(session);
        drop(cassie);
    }

    // Act
    let cassie = Cassie::new_with_data_dir(&path).expect("reopen Cassie instance");
    cassie.startup().expect("recover collected statistics");
    let session = cassie.create_session("tester", None);
    let statistics = cassie
        .execute_sql(
            &session,
            "SELECT s.stxname, d.stxdndistinct IS NOT NULL, \
             d.stxddependencies IS NOT NULL, d.stxdmcv IS NOT NULL \
             FROM pg_catalog.pg_statistic_ext AS s \
             JOIN pg_catalog.pg_statistic_ext_data AS d ON d.stxoid = s.oid \
             WHERE s.stxname = 'compat_extended_stats'",
            vec![],
        )
        .expect("read persisted extended-statistics data")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(statistics, expected);
}
