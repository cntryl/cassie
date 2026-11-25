//! Persistence of the staging-table rename-swap used by projection rebuilds.
use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::types::Value;

fn execute(cassie: &Cassie, session: &cassie::app::CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|error| panic!("query failed: {sql}: {error}"));
}

fn rows(cassie: &Cassie, session: &cassie::app::CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|error| panic!("query failed: {sql}: {error}"))
        .rows()
        .to_vec()
}

fn catalog(cassie: &Cassie, session: &cassie::app::CassieSession) -> Vec<Vec<Value>> {
    rows(
        cassie,
        session,
        "SELECT relname, oid::BIGINT FROM pg_catalog.pg_class \
         WHERE relname IN ('rebuild_live', 'rebuild_previous', 'rebuild_staging') \
         ORDER BY relname",
    )
}

fn copy_and_swap(cassie: &Cassie, session: &cassie::app::CassieSession) {
    cassie
        .copy_from_csv_stdin(
            session,
            &CopyStatement {
                table: "rebuild_staging".to_owned(),
                columns: vec!["id".to_owned(), "label".to_owned()],
                format: CopyFormat::Csv,
                header: false,
            },
            b"2,rebuilt\n",
        )
        .expect("COPY replacement row into staging table");
    execute(
        cassie,
        session,
        "ALTER TABLE rebuild_live RENAME TO rebuild_previous",
    );
    execute(
        cassie,
        session,
        "ALTER TABLE rebuild_staging RENAME TO rebuild_live",
    );
}

fn assert_swapped_state(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    expected_catalog: &[Vec<Value>],
) {
    assert_eq!(
        rows(cassie, session, "SELECT id, label FROM rebuild_live"),
        vec![vec![Value::Int64(2), Value::String("rebuilt".to_owned())]]
    );
    assert_eq!(
        rows(cassie, session, "SELECT id, label FROM rebuild_previous"),
        vec![vec![Value::Int64(1), Value::String("old".to_owned())]]
    );
    assert_eq!(catalog(cassie, session), expected_catalog);
}

fn assert_original_state(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    expected_catalog: &[Vec<Value>],
) {
    assert_eq!(
        rows(cassie, session, "SELECT id, label FROM rebuild_live"),
        vec![vec![Value::Int64(1), Value::String("old".to_owned())]]
    );
    assert_eq!(
        rows(cassie, session, "SELECT id, label FROM rebuild_staging"),
        Vec::<Vec<Value>>::new()
    );
    assert_eq!(catalog(cassie, session), expected_catalog);
}

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

#[test]
fn should_recover_copy_loaded_rebuild_swap_after_local_restart() {
    // Arrange
    let directory =
        crate::support_catalog_directory::CatalogDirectory::new("copy-rebuild-swap-restart");
    let original_catalog;
    let committed_catalog;
    {
        let cassie =
            Cassie::new_with_data_dir_and_config(&directory.0, CassieRuntimeConfig::default())
                .expect("create local Cassie");
        cassie.startup().expect("start local Cassie");
        let owner = cassie.create_session("owner", None);
        let observer = cassie.create_session("observer", None);
        execute(
            &cassie,
            &owner,
            "CREATE TABLE rebuild_live (id INT, label TEXT)",
        );
        execute(
            &cassie,
            &owner,
            "CREATE TABLE rebuild_staging (id INT, label TEXT)",
        );
        execute(
            &cassie,
            &owner,
            "INSERT INTO rebuild_live VALUES (1, 'old')",
        );
        original_catalog = catalog(&cassie, &observer);

        // Act
        execute(&cassie, &owner, "BEGIN");
        copy_and_swap(&cassie, &owner);
        let owner_catalog = catalog(&cassie, &owner);
        let observer_rows = rows(&cassie, &observer, "SELECT id, label FROM rebuild_live");
        let observer_staging = rows(&cassie, &observer, "SELECT id, label FROM rebuild_staging");
        assert_swapped_state(&cassie, &owner, &owner_catalog);
        execute(&cassie, &owner, "ROLLBACK");

        // Assert rollback, then retry and commit the same COPY-loaded swap.
        assert_eq!(
            observer_rows,
            vec![vec![Value::Int64(1), Value::String("old".to_owned())]]
        );
        assert_eq!(observer_staging, Vec::<Vec<Value>>::new());
        assert_original_state(&cassie, &owner, &original_catalog);
        assert_original_state(&cassie, &observer, &original_catalog);

        execute(&cassie, &owner, "BEGIN");
        copy_and_swap(&cassie, &owner);
        execute(&cassie, &owner, "COMMIT");
        committed_catalog = catalog(&cassie, &observer);
        assert_swapped_state(&cassie, &observer, &committed_catalog);
    }

    let cassie = Cassie::new_with_data_dir_and_config(&directory.0, CassieRuntimeConfig::default())
        .expect("reopen local Cassie");
    cassie.startup().expect("recover local Cassie");
    let observer = cassie.create_session("observer", None);

    // Assert
    assert_swapped_state(&cassie, &observer, &committed_catalog);
    assert_ne!(original_catalog, committed_catalog);
}
