//! Persistence of the staging-table rename-swap used by projection rebuilds.
use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

#[test]
fn should_recover_swapped_rebuild_tables_after_local_restart() {
    // Arrange
    let directory = crate::support_catalog_directory::CatalogDirectory::new("rebuild-swap-restart");
    let before_oids;
    {
        let cassie =
            Cassie::new_with_data_dir_and_config(&directory.0, CassieRuntimeConfig::default())
                .expect("create local Cassie");
        cassie.startup().expect("start local Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE rebuild_live (id INT, label TEXT)",
            "CREATE TABLE rebuild_staging (id INT, label TEXT)",
            "INSERT INTO rebuild_live VALUES (1, 'old')",
            "INSERT INTO rebuild_staging VALUES (2, 'rebuilt')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }
        before_oids = cassie
            .execute_sql(
                &session,
                "SELECT relname, oid::BIGINT FROM pg_catalog.pg_class \
                 WHERE relname IN ('rebuild_live', 'rebuild_staging') ORDER BY relname",
                vec![],
            )
            .expect("read original relation identities")
            .rows()
            .to_vec();
        assert_eq!(before_oids.len(), 2);
        assert_ne!(before_oids[0][1], before_oids[1][1]);

        // Act
        for sql in [
            "BEGIN",
            "ALTER TABLE rebuild_live RENAME TO rebuild_previous",
            "ALTER TABLE rebuild_staging RENAME TO rebuild_live",
            "COMMIT",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }
    }

    let cassie = Cassie::new_with_data_dir_and_config(&directory.0, CassieRuntimeConfig::default())
        .expect("reopen local Cassie");
    cassie.startup().expect("recover local Cassie");
    let session = cassie.create_session("tester", None);
    let rows = |sql: &str| {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect(sql)
            .rows()
            .to_vec()
    };

    // Assert
    assert_eq!(
        rows("SELECT id, label FROM rebuild_live"),
        vec![vec![Value::Int64(2), Value::String("rebuilt".to_owned())]]
    );
    assert_eq!(
        rows("SELECT id, label FROM rebuild_previous"),
        vec![vec![Value::Int64(1), Value::String("old".to_owned())]]
    );
    assert_eq!(
        rows(
            "SELECT relname, oid::BIGINT FROM pg_catalog.pg_class \
              WHERE relname IN ('rebuild_live', 'rebuild_previous', 'rebuild_staging') \
              ORDER BY relname"
        ),
        vec![
            vec![
                Value::String("rebuild_live".to_owned()),
                before_oids[1][1].clone()
            ],
            vec![
                Value::String("rebuild_previous".to_owned()),
                before_oids[0][1].clone()
            ],
        ]
    );
}
