//! Logical images reject source-generation changes instead of publishing mixed rows.
use super::support_database_image_generation::Fixture;
use cassie::types::Value;

#[test]
fn should_reject_source_update_before_the_first_database_image_chunk() {
    // Arrange
    let fixture = Fixture::seeded();
    let mut backup = fixture
        .cassie
        .begin_database_backup("analytics")
        .expect("backup");
    // Act
    fixture
        .cassie
        .execute_sql(&fixture.session, "UPDATE docs SET v = 2", vec![])
        .expect("update");
    // Assert
    Fixture::assert_rejected(&mut backup);
}

#[test]
fn should_reject_schema_change_after_database_image_catalog_capture() {
    // Arrange
    let fixture = Fixture::seeded();
    let mut backup = fixture
        .cassie
        .begin_database_backup("analytics")
        .expect("backup");
    let prefix = backup.next_chunk().expect("first chunk").expect("prefix");
    // Act
    fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "ALTER TABLE docs ADD COLUMN added TEXT",
            vec![],
        )
        .expect("DDL");
    // Assert
    Fixture::assert_rejected(&mut backup);
    fixture.assert_incomplete_image(&prefix);
}

#[test]
fn should_reject_replaced_database_family_before_image_stream_admission() {
    // Arrange
    let fixture = Fixture::seeded();
    fixture
        .cassie
        .midge
        .create_database("empty", None)
        .expect("empty database");
    let old = fixture
        .cassie
        .midge
        .get_database("empty")
        .expect("old lookup")
        .expect("old database");
    let mut backup = fixture
        .cassie
        .begin_database_backup("empty")
        .expect("backup");
    // Act
    fixture
        .cassie
        .midge
        .drop_database("empty")
        .expect("drop empty");
    fixture
        .cassie
        .midge
        .create_database("empty", None)
        .expect("replacement");
    // Assert
    let new = fixture
        .cassie
        .midge
        .get_database("empty")
        .expect("new lookup")
        .expect("new database");
    assert_ne!(old.physical_family, new.physical_family);
    Fixture::assert_rejected(&mut backup);
}

#[test]
fn should_restore_unchanged_paged_database_image_exactly() {
    // Arrange
    let fixture = Fixture::seeded();
    let mut backup = fixture
        .cassie
        .begin_database_backup("analytics")
        .expect("backup");
    let mut image = Vec::new();
    let mut chunks = 0;
    // Act
    while let Some(chunk) = backup.next_chunk().expect("consistent stream") {
        assert!(chunk.len() <= 64 * 1024);
        image.extend(chunk);
        chunks += 1;
    }
    let mut restore = fixture
        .cassie
        .begin_database_restore("restored")
        .expect("restore");
    for chunk in image.chunks(3001) {
        restore
            .push_chunk(chunk)
            .expect("frame/count/checksum validation");
    }
    restore.finish().expect("complete image");
    let session = fixture
        .cassie
        .create_session("tester", Some("restored".to_owned()));
    let result = fixture
        .cassie
        .execute_sql(&session, "SELECT v FROM docs", vec![])
        .expect("restored query");
    // Assert
    assert!(chunks > 1);
    assert_eq!(result.rows, vec![vec![Value::Int64(1)]; 300]);
    assert_eq!(result.columns[0].type_oid, 20);
    assert!(backup.next_chunk().expect("completed EOF").is_none());
}

#[test]
fn should_reject_atomic_source_update_between_paged_database_image_chunks() {
    // Arrange
    let fixture = Fixture::seeded();
    let mut backup = fixture
        .cassie
        .begin_database_backup("analytics")
        .expect("backup");
    let prefix = backup.next_chunk().expect("first chunk").expect("prefix");
    assert_eq!(prefix.len(), 64 * 1024);
    let generation = fixture
        .cassie
        .midge
        .collection_generation("analytics.public.docs")
        .expect("generation");

    // Act
    let update = fixture
        .cassie
        .execute_sql(&fixture.session, "UPDATE docs SET v = 2", vec![])
        .expect("atomic update");

    // Assert
    assert_eq!(update.command, "UPDATE 300");
    assert_eq!(
        fixture
            .cassie
            .midge
            .collection_generation("analytics.public.docs")
            .expect("changed generation"),
        generation + 1
    );
    Fixture::assert_rejected(&mut backup);
    fixture.assert_incomplete_image(&prefix);
    let source = fixture
        .cassie
        .execute_sql(&fixture.session, "SELECT v FROM docs", vec![])
        .expect("source preserved");
    assert_eq!(source.rows, vec![vec![Value::Int64(2)]; 300]);
}

#[test]
fn should_preserve_a_database_created_while_its_restore_is_staged() {
    // Arrange
    let fixture = Fixture::seeded();
    let mut backup = fixture
        .cassie
        .begin_database_backup("analytics")
        .expect("backup");
    let mut restore = fixture
        .cassie
        .begin_database_restore("contested")
        .expect("restore");
    while let Some(chunk) = backup.next_chunk().expect("unchanged image") {
        restore.push_chunk(&chunk).expect("valid image chunk");
    }
    fixture
        .cassie
        .midge
        .create_database("contested", None)
        .expect("concurrent publication");
    let owner = fixture
        .cassie
        .midge
        .get_database("contested")
        .expect("lookup")
        .expect("published database")
        .physical_family;
    // Act
    let result = restore.finish();
    // Assert
    assert!(
        result.is_err(),
        "restore must not overwrite another published owner"
    );
    assert_eq!(
        fixture
            .cassie
            .midge
            .get_database("contested")
            .expect("lookup")
            .expect("preserved database")
            .physical_family,
        owner
    );
    restore.abort().expect("cleanup");
    restore.abort().expect("idempotent cleanup");
}
