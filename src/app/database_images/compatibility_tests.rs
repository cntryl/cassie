use super::{Cassie, DatabaseBackupStream, DATABASE_IMAGE_MAGIC, IMAGE_CHUNK_BYTES};

const V1: &str = "cassie-midge-layout-v1-0.1.0";
const V2: &str = "cassie-midge-layout-v2-0.2.0";

fn fixture() -> Cassie {
    let cassie = Cassie::new_in_memory_with_config(crate::config::CassieRuntimeConfig::default())
        .expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE docs (id INT, body TEXT)", vec![])
        .expect("table");
    cassie
        .execute_sql(&session, "INSERT INTO docs VALUES (7,'v1 row')", vec![])
        .expect("row");
    cassie
}

fn collect(mut backup: DatabaseBackupStream) -> Vec<u8> {
    let mut image = Vec::new();
    while let Some(chunk) = backup.next_chunk().expect("image chunk") {
        image.extend_from_slice(&chunk);
    }
    image
}

fn rewrite_frame(image: &[u8], tag: u8, change: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let mut start = DATABASE_IMAGE_MAGIC.len();
    while start < image.len() {
        let length = usize::try_from(u32::from_be_bytes(
            image[start..start + 4].try_into().expect("length"),
        ))
        .expect("frame length");
        let end = start + 4 + length;
        if image[start + 4] == tag {
            let mut value: serde_json::Value =
                serde_json::from_slice(&image[start + 5..end]).expect("JSON frame");
            change(&mut value);
            let payload = serde_json::to_vec(&value).expect("encode changed frame");
            let mut changed = image[..start].to_vec();
            changed.extend_from_slice(
                &u32::try_from(payload.len() + 1)
                    .expect("small frame")
                    .to_be_bytes(),
            );
            changed.push(tag);
            changed.extend_from_slice(&payload);
            changed.extend_from_slice(&image[end..]);
            return changed;
        }
        start = end;
    }
    panic!("missing image frame");
}

fn with_layout(image: &[u8], layout: &str) -> Vec<u8> {
    rewrite_frame(image, b'H', |header| {
        header["storage_layout_version"] = layout.into()
    })
}

fn header_layout(image: &[u8]) -> String {
    let start = DATABASE_IMAGE_MAGIC.len();
    let length = usize::try_from(u32::from_be_bytes(
        image[start..start + 4].try_into().expect("length"),
    ))
    .expect("frame length");
    let header: serde_json::Value =
        serde_json::from_slice(&image[start + 5..start + 4 + length]).expect("header");
    header["storage_layout_version"]
        .as_str()
        .expect("layout")
        .to_owned()
}

#[test]
fn should_preserve_v1_compatibility_through_an_image_round_trip() {
    // Arrange
    let cassie = fixture();
    let image = collect(cassie.begin_database_backup("postgres").expect("backup"));
    // Act
    let mut restore = cassie.begin_database_restore("restored").expect("staging");
    restore.push_chunk(&image).expect("complete image");
    restore.finish().expect("publish v1 image");
    // Assert
    assert_eq!(header_layout(&image), V1);
    assert_eq!(cassie.midge.storage_layout_version().expect("version"), V1);
    assert!(cassie
        .midge
        .get_database("restored")
        .expect("target")
        .is_some());
}

#[test]
fn should_export_the_actual_upgraded_store_requirement() {
    // Arrange
    let cassie = fixture();
    cassie
        .midge
        .require_storage_layout_version(V2)
        .expect("upgrade");
    // Act
    let image = collect(cassie.begin_database_backup("postgres").expect("backup"));
    // Assert
    assert_eq!(header_layout(&image), V2);
}

#[test]
fn should_reject_a_marker_transition_after_an_actual_backup_chunk() {
    // Arrange
    let cassie = fixture();
    cassie
        .midge
        .put_document(
            "postgres.public.docs",
            Some("wide".to_owned()),
            serde_json::json!({"id":8,"body":"x".repeat(IMAGE_CHUNK_BYTES + 1)}),
        )
        .expect("one chunk-boundary row");
    let mut backup = cassie.begin_database_backup("postgres").expect("backup");
    let first = backup.next_chunk().expect("first chunk").expect("bytes");
    assert_eq!(first.len(), IMAGE_CHUNK_BYTES);
    cassie
        .midge
        .require_storage_layout_version(V2)
        .expect("upgrade between chunks");
    // Act
    let result = backup.next_chunk();
    // Assert
    assert!(
        result.is_err(),
        "mixed compatibility generations cannot produce an image footer"
    );
    assert!(backup.next_chunk().is_err(), "failure remains terminal");
}

#[test]
fn should_require_v2_before_publishing_a_validated_restore() {
    // Arrange
    let cassie = fixture();
    let image = with_layout(
        &collect(cassie.begin_database_backup("postgres").expect("backup")),
        V2,
    );
    let mut restore = cassie.begin_database_restore("restored").expect("staging");
    restore
        .push_chunk(&image)
        .expect("complete validated image");
    assert_eq!(
        cassie
            .midge
            .storage_layout_version()
            .expect("before publication"),
        V1
    );
    assert!(cassie
        .midge
        .get_database("restored")
        .expect("invisible target")
        .is_none());
    // Act
    restore.finish().expect("publish");
    // Assert
    assert_eq!(
        cassie
            .midge
            .storage_layout_version()
            .expect("after publication"),
        V2
    );
    let session = cassie.create_session("tester", Some("restored".to_owned()));
    let rows = cassie
        .execute_sql(&session, "SELECT id::TEXT,body FROM docs", vec![])
        .expect("v1 encoded row in restored v2 store");
    assert_eq!(rows.rows().len(), 1);
    assert_eq!(
        rows.rows()[0][0],
        crate::types::Value::String("7".to_owned())
    );
    assert_eq!(
        rows.rows()[0][1],
        crate::types::Value::String("v1 row".to_owned())
    );
}

#[test]
fn should_keep_the_v2_requirement_when_restore_publication_fails() {
    // Arrange
    let cassie = fixture();
    let image = with_layout(
        &collect(cassie.begin_database_backup("postgres").expect("backup")),
        V2,
    );
    let mut restore = cassie.begin_database_restore("contested").expect("staging");
    restore.push_chunk(&image).expect("validated image");
    cassie
        .midge
        .create_database("contested", Some("existing winner".to_owned()))
        .expect("publish competing name");
    // Act
    let result = restore.finish();
    // Assert
    assert!(
        result.is_err(),
        "restore cannot replace the competing database"
    );
    assert_eq!(
        cassie
            .midge
            .storage_layout_version()
            .expect("acknowledged before failed publication"),
        V2
    );
    let database = cassie
        .midge
        .get_database("contested")
        .expect("winner lookup")
        .expect("winner");
    assert_eq!(database.description.as_deref(), Some("existing winner"));
}

#[test]
fn should_keep_restore_staging_abortable_when_marker_admission_fails() {
    // Arrange
    use crate::midge::adapter::StorageFamily;
    let cassie = fixture();
    let image = with_layout(
        &collect(cassie.begin_database_backup("postgres").expect("backup")),
        V2,
    );
    let mut restore = cassie.begin_database_restore("rejected").expect("staging");
    restore.push_chunk(&image).expect("validated image");
    let key = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Schema, b"")
        .expect("schema")
        .into_iter()
        .find_map(|(key, value)| (value == V1.as_bytes()).then_some(key))
        .expect("marker key");
    cassie
        .midge
        .raw_put(StorageFamily::Schema, &key, b"unknown-reader")
        .expect("make marker inadmissible");
    // Act
    let result = restore.finish();
    restore
        .abort()
        .expect("failed gate retains abortable staging");
    // Assert
    assert!(result.is_err());
    assert!(cassie
        .midge
        .get_database("rejected")
        .expect("target")
        .is_none());
    assert_eq!(
        cassie
            .midge
            .raw_get(StorageFamily::Schema, &key)
            .expect("unchanged marker"),
        Some(b"unknown-reader".to_vec())
    );
}

#[test]
fn should_leave_v1_unchanged_for_incomplete_or_corrupt_v2_images() {
    // Arrange
    for corrupt in [false, true] {
        let cassie = fixture();
        let complete = with_layout(
            &collect(cassie.begin_database_backup("postgres").expect("backup")),
            V2,
        );
        let image = if corrupt {
            rewrite_frame(&complete, b'F', |footer| {
                footer["checksum_sha256"] = "0".repeat(64).into()
            })
        } else {
            complete[..complete.len() - 1].to_vec()
        };
        let mut restore = cassie.begin_database_restore("rejected").expect("staging");
        // Act
        let push = restore.push_chunk(&image);
        let finish = restore.finish();
        restore.abort().expect("abort invisible staging");
        // Assert
        assert!(push.is_err() || finish.is_err());
        assert!(finish.is_err(), "a failed image cannot publish");
        assert_eq!(
            cassie
                .midge
                .storage_layout_version()
                .expect("unchanged version"),
            V1
        );
        assert!(cassie
            .midge
            .get_database("rejected")
            .expect("target")
            .is_none());
    }
}

#[test]
fn should_reject_an_unknown_image_requirement_without_upgrading_or_publishing() {
    // Arrange
    let cassie = fixture();
    let image = with_layout(
        &collect(cassie.begin_database_backup("postgres").expect("backup")),
        "cassie-midge-layout-v99",
    );
    let mut restore = cassie.begin_database_restore("rejected").expect("staging");
    // Act
    let result = restore.push_chunk(&image);
    restore.abort().expect("abort");
    // Assert
    assert!(result.is_err());
    assert_eq!(cassie.midge.storage_layout_version().expect("version"), V1);
    assert!(cassie
        .midge
        .get_database("rejected")
        .expect("target")
        .is_none());
}

#[test]
fn should_preserve_v2_when_restoring_a_v1_image() {
    // Arrange
    let cassie = fixture();
    let image = collect(cassie.begin_database_backup("postgres").expect("v1 backup"));
    cassie
        .midge
        .require_storage_layout_version(V2)
        .expect("upgrade destination");
    let mut restore = cassie.begin_database_restore("restored").expect("staging");
    restore
        .push_chunk(&image)
        .expect("v1 image remains readable");
    // Act
    restore.finish().expect("publish");
    // Assert
    assert_eq!(cassie.midge.storage_layout_version().expect("version"), V2);
    assert!(cassie
        .midge
        .get_database("restored")
        .expect("target")
        .is_some());
}

#[test]
fn should_retain_validated_restore_ownership_after_storage_requires_recovery() {
    // Arrange
    for layout in [V1, V2] {
        let cassie = fixture();
        let image = with_layout(
            &collect(cassie.begin_database_backup("postgres").expect("backup")),
            layout,
        );
        let mut restore = cassie
            .begin_database_restore("quarantined_restore")
            .expect("staging");
        restore.push_chunk(&image).expect("validated image");
        let catalog_entries = restore.catalog_entries.len();
        cassie.midge.latch_storage_recovery_required_for_test();

        // Act
        let failure = restore.finish();

        // Assert
        assert_eq!(
            failure
                .expect_err("publication not admitted")
                .descriptor()
                .sql_state,
            "58030"
        );
        assert!(restore.staged.is_some());
        assert!(matches!(restore.phase, super::RestorePhase::Footer));
        assert_eq!(restore.catalog_entries.len(), catalog_entries);
        assert!(!cassie.catalog.database_exists("quarantined_restore"));
    }
}
