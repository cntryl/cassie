//! Pending name-only cleanup must not delete a replacement scalar index.
use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_reject_replacement_while_drop_is_pending_in_savepoint() {
    // Arrange
    let fixture = sql_fixture(
        "index-replacement-guard-savepoint",
        &[
            "CREATE TABLE guarded_rows (id INT, payload INT)",
            "CREATE UNIQUE INDEX guarded_idx ON guarded_rows (id)",
            "BEGIN",
            "SAVEPOINT before_drop",
            "DROP INDEX guarded_idx",
        ],
    );

    // Act
    let rejected = fixture.execute("CREATE UNIQUE INDEX guarded_idx ON guarded_rows (payload)");
    fixture.execute("ROLLBACK TO before_drop").unwrap();
    fixture.execute("COMMIT").unwrap();

    // Assert
    let message = rejected
        .expect_err("same-name replacement must be rejected")
        .to_string();
    assert!(message.contains("pending index cleanup"), "{message}");
    fixture
        .execute("INSERT INTO guarded_rows VALUES (1, 10)")
        .unwrap();
    assert!(fixture
        .execute("INSERT INTO guarded_rows VALUES (1, 11)")
        .is_err());
    fixture
        .execute("INSERT INTO guarded_rows VALUES (2, 10)")
        .unwrap();
}

#[test]
fn should_reject_committed_pending_cleanup_but_allow_other_names() {
    // Arrange
    let fixture = sql_fixture(
        "index-replacement-guard-pending",
        &[
            "CREATE TABLE guarded_rows (id INT, payload INT)",
            "CREATE UNIQUE INDEX guarded_idx ON guarded_rows (id)",
        ],
    );
    let old_reader = fixture.cassie.begin_schema_epoch_guard_for_diagnostics();
    fixture.execute("BEGIN").unwrap();
    fixture.execute("DROP INDEX guarded_idx").unwrap();
    fixture.execute("COMMIT").unwrap();

    // Act
    let rejected = fixture.execute("CREATE UNIQUE INDEX guarded_idx ON guarded_rows (payload)");
    let unrelated = fixture.execute("CREATE INDEX another_idx ON guarded_rows (payload)");

    // Assert
    let message = rejected.expect_err("cleanup remains pending").to_string();
    assert!(message.contains("pending index cleanup"), "{message}");
    unrelated.expect("another index name remains available");
    drop(old_reader);
    fixture
        .cassie
        .run_deferred_schema_cleanup_for_diagnostics()
        .unwrap();
    fixture
        .execute("CREATE UNIQUE INDEX guarded_idx ON guarded_rows (payload)")
        .unwrap();
}

#[test]
fn should_reject_renaming_another_index_into_a_pending_name() {
    // Arrange
    let fixture = sql_fixture(
        "index-replacement-guard-rename",
        &[
            "CREATE TABLE guarded_rows (id INT, payload INT)",
            "CREATE INDEX guarded_idx ON guarded_rows (id)",
            "CREATE INDEX other_idx ON guarded_rows (payload)",
            "BEGIN",
            "DROP INDEX guarded_idx",
            "SAVEPOINT before_rename",
        ],
    );

    // Act
    let rejected = fixture.execute("ALTER INDEX other_idx RENAME TO guarded_idx");
    fixture.execute("ROLLBACK TO before_rename").unwrap();
    fixture.execute("ROLLBACK").unwrap();

    // Assert
    let message = rejected
        .expect_err("pending cleanup owns destination")
        .to_string();
    assert!(message.contains("pending index cleanup"), "{message}");
    fixture
        .execute("ALTER INDEX other_idx RENAME TO another_idx")
        .unwrap();
}

#[test]
fn should_reject_a_vector_index_using_a_pending_scalar_name() {
    // Arrange
    let fixture = sql_fixture(
        "index-replacement-guard-vector",
        &[
            "CREATE TABLE guarded_rows (id INT, content TEXT, embedding VECTOR(2))",
            "CREATE INDEX guarded_idx ON guarded_rows (id)",
        ],
    );
    let old_reader = fixture.cassie.begin_schema_epoch_guard_for_diagnostics();
    fixture.execute("BEGIN").unwrap();
    fixture.execute("DROP INDEX guarded_idx").unwrap();
    fixture.execute("COMMIT").unwrap();

    // Act
    let rejected = fixture.execute(
        "CREATE INDEX guarded_idx ON guarded_rows USING vector (embedding) \
         WITH (source_field = content, metric = l2)",
    );

    // Assert
    let message = rejected
        .expect_err("pending scalar cleanup owns name")
        .to_string();
    assert!(message.contains("pending index cleanup"), "{message}");
    drop(old_reader);
}

#[test]
fn should_revalidate_new_cleanup_before_publishing_a_staged_index() {
    // Arrange
    let fixture = sql_fixture(
        "index-replacement-guard-commit-create",
        &["CREATE TABLE guarded_rows (id INT)"],
    );
    let old_reader = fixture.cassie.begin_schema_epoch_guard_for_diagnostics();
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("CREATE INDEX guarded_idx ON guarded_rows (id)")
        .unwrap();
    fixture
        .cassie
        .midge
        .defer_drop_index("guarded_rows", "guarded_idx", 0)
        .expect("concurrent marker");

    // Act
    let commit = fixture.execute("COMMIT");
    fixture.execute("ROLLBACK").unwrap();

    // Assert
    assert!(
        commit.is_err(),
        "stale staged CREATE cannot publish over a cleanup marker"
    );
    assert!(fixture
        .cassie
        .midge
        .get_index("guarded_rows", "guarded_idx")
        .expect("read")
        .is_none());
    drop(old_reader);
}

#[test]
fn should_revalidate_index_metadata_before_publishing_a_staged_drop() {
    // Arrange
    let fixture = sql_fixture(
        "index-replacement-guard-commit-drop",
        &[
            "CREATE TABLE guarded_rows (id INT, payload INT)",
            "CREATE INDEX guarded_idx ON guarded_rows (id)",
        ],
    );
    let old_reader = fixture.cassie.begin_schema_epoch_guard_for_diagnostics();
    fixture.execute("BEGIN").unwrap();
    fixture.execute("DROP INDEX guarded_idx").unwrap();
    let mut changed = fixture
        .cassie
        .midge
        .get_index("guarded_rows", "guarded_idx")
        .expect("read")
        .expect("old index");
    changed.field = "payload".to_owned();
    changed.fields = vec!["payload".to_owned()];
    fixture
        .cassie
        .midge
        .put_index(&changed)
        .expect("concurrent adapter change");

    // Act
    let commit = fixture.execute("COMMIT");
    fixture.execute("ROLLBACK").unwrap();

    // Assert
    assert!(
        commit.is_err(),
        "stale staged DROP cannot publish a cleanup for changed metadata"
    );
    assert_eq!(
        fixture
            .cassie
            .midge
            .get_index("guarded_rows", "guarded_idx")
            .expect("read")
            .expect("index survives")
            .field,
        "payload"
    );
    drop(old_reader);
}

#[test]
fn should_allow_creating_then_dropping_a_new_index_in_one_transaction() {
    // Arrange
    let fixture = sql_fixture(
        "index-replacement-guard-create-drop",
        &["CREATE TABLE guarded_rows (id INT)"],
    );
    let old_reader = fixture.cassie.begin_schema_epoch_guard_for_diagnostics();
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("CREATE INDEX guarded_idx ON guarded_rows (id)")
        .unwrap();

    // Act
    fixture.execute("DROP INDEX guarded_idx").unwrap();
    fixture.execute("COMMIT").unwrap();

    // Assert
    drop(old_reader);
    fixture
        .cassie
        .run_deferred_schema_cleanup_for_diagnostics()
        .unwrap();
    assert!(fixture
        .cassie
        .midge
        .get_index("guarded_rows", "guarded_idx")
        .expect("read")
        .is_none());
    fixture
        .execute("CREATE INDEX guarded_idx ON guarded_rows (id)")
        .unwrap();
}

#[test]
fn should_allow_creating_then_renaming_a_new_index_in_one_transaction() {
    // Arrange
    let fixture = sql_fixture(
        "index-guard-create-rename",
        &["CREATE TABLE guarded_rows (id INT)"],
    );
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("CREATE INDEX first_idx ON guarded_rows (id)")
        .unwrap();

    // Act
    fixture
        .execute("ALTER INDEX first_idx RENAME TO second_idx")
        .unwrap();
    fixture.execute("COMMIT").unwrap();

    // Assert
    assert!(fixture
        .cassie
        .midge
        .get_index("guarded_rows", "first_idx")
        .expect("read old")
        .is_none());
    assert!(fixture
        .cassie
        .midge
        .get_index("guarded_rows", "second_idx")
        .expect("read new")
        .is_some());
}

#[test]
fn should_allow_repeated_quoted_renames_of_a_new_index_in_one_transaction() {
    // Arrange
    let fixture = sql_fixture(
        "index-guard-create-quoted-renames",
        &["CREATE TABLE guarded_rows (id INT)"],
    );
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("CREATE INDEX first_idx ON guarded_rows (id)")
        .unwrap();

    // Act
    fixture
        .execute("ALTER INDEX first_idx RENAME TO \"Renamed.Index\"")
        .unwrap();
    fixture
        .execute("ALTER INDEX \"Renamed.Index\" RENAME TO final_idx")
        .unwrap();
    fixture.execute("COMMIT").unwrap();

    // Assert
    assert!(fixture
        .cassie
        .midge
        .get_index("guarded_rows", "final_idx")
        .expect("read")
        .is_some());
    assert!(fixture
        .cassie
        .midge
        .get_index("guarded_rows", "first_idx")
        .expect("read")
        .is_none());
}

#[test]
fn should_restore_a_renamed_existing_index_after_rolling_back_its_drop() {
    // Arrange
    let fixture = sql_fixture(
        "index-guard-rename-drop-savepoint",
        &[
            "CREATE TABLE guarded_rows (id INT)",
            "CREATE INDEX old_idx ON guarded_rows (id)",
            "BEGIN",
            "ALTER INDEX old_idx RENAME TO renamed_idx",
            "SAVEPOINT before_drop",
            "DROP INDEX renamed_idx",
        ],
    );

    // Act
    fixture.execute("ROLLBACK TO before_drop").unwrap();
    fixture.execute("COMMIT").unwrap();

    // Assert
    assert!(fixture
        .cassie
        .midge
        .get_index("guarded_rows", "renamed_idx")
        .expect("read")
        .is_some());
    assert!(fixture
        .cassie
        .midge
        .get_index("guarded_rows", "old_idx")
        .expect("read")
        .is_none());
}

#[test]
fn should_reject_a_staged_create_when_a_same_name_journal_appears_before_commit() {
    // Arrange
    let fixture = sql_fixture(
        "index-guard-journal-create",
        &[
            "CREATE TABLE guarded_rows (id INT)",
            "CREATE INDEX seed_idx ON guarded_rows (id)",
        ],
    );
    let mut prepared = fixture
        .cassie
        .midge
        .get_index("guarded_rows", "seed_idx")
        .expect("read")
        .expect("seed");
    prepared.name = "guarded_idx".to_owned();
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("CREATE INDEX guarded_idx ON guarded_rows (id)")
        .unwrap();
    cassie::midge::adapter::set_index_publication_failure_point(true);
    let failed_prepare = fixture.cassie.midge.put_index(&prepared);
    cassie::midge::adapter::set_index_publication_failure_point(false);
    assert!(failed_prepare.is_err());

    // Act
    let committed = fixture.execute("COMMIT");
    fixture.execute("ROLLBACK").unwrap();

    // Assert
    assert!(
        committed.is_err(),
        "journal appeared after CREATE was staged"
    );
    fixture
        .cassie
        .midge
        .replay_pending_index_publications()
        .expect("recover journal");
}

#[test]
fn should_reject_a_staged_rename_when_a_source_journal_appears_before_commit() {
    // Arrange
    let fixture = sql_fixture(
        "index-guard-journal-rename",
        &[
            "CREATE TABLE guarded_rows (id INT)",
            "CREATE INDEX old_idx ON guarded_rows (id)",
        ],
    );
    let prepared = fixture
        .cassie
        .midge
        .get_index("guarded_rows", "old_idx")
        .expect("read")
        .expect("old");
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("ALTER INDEX old_idx RENAME TO new_idx")
        .unwrap();
    cassie::midge::adapter::set_index_publication_failure_point(true);
    let failed_prepare = fixture.cassie.midge.put_index(&prepared);
    cassie::midge::adapter::set_index_publication_failure_point(false);
    assert!(failed_prepare.is_err());

    // Act
    let committed = fixture.execute("COMMIT");
    fixture.execute("ROLLBACK").unwrap();

    // Assert
    assert!(
        committed.is_err(),
        "source journal appeared after rename was staged"
    );
    fixture
        .cassie
        .midge
        .replay_pending_index_publications()
        .expect("recover journal");
    assert!(fixture
        .cassie
        .midge
        .get_index("guarded_rows", "old_idx")
        .expect("read")
        .is_some());
}
