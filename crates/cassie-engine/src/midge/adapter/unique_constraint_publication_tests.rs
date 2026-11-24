//! Engine startup replays the concrete adapter's pending uniqueness records.

use cassie_sql::declarations::FieldConstraint;
use cassie_storage::StorageError;

use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use crate::types::{DataType, FieldSchema, Schema};

const CANONICAL: &str = "postgres.public.unique_publication_recovery";

#[test]
fn should_replay_canonical_unique_publication_given_a_prepared_record_after_restart() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie-unique-publication-recovery-{}",
        uuid::Uuid::new_v4()
    ));
    let cassie =
        Cassie::new_local_with_data_dir_and_config(&path, CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    cassie.midge.create_collection(CANONICAL, schema()).unwrap();
    cassie
        .midge
        .put_document(
            CANONICAL,
            Some("existing".into()),
            serde_json::json!({"value":"same"}),
        )
        .unwrap();
    let mut constraint = FieldConstraint::new("value");
    constraint.unique = true;
    constraint.unique_name = Some("recovered_unique".into());
    cassie
        .midge
        .seed_pending_unique_constraint_publication(
            CANONICAL,
            vec![constraint.clone()],
            vec![constraint.clone()],
        )
        .unwrap();
    drop(cassie);

    // Act
    let restarted =
        Cassie::new_local_with_data_dir_and_config(&path, CassieRuntimeConfig::default()).unwrap();
    restarted.startup().unwrap();
    let metadata = restarted.midge.load_constraints(CANONICAL).unwrap();
    let duplicate = restarted.midge.put_document(
        CANONICAL,
        Some("duplicate".into()),
        serde_json::json!({"value":"same"}),
    );
    let pending = restarted
        .midge
        .pending_unique_constraint_publication_count()
        .unwrap();

    // Assert
    assert_eq!(metadata, vec![constraint]);
    assert!(
        matches!(&duplicate, Err(StorageError::UniqueViolation { constraint, .. }) if constraint == "recovered_unique"),
        "replayed publication must reject the duplicate: {duplicate:?}"
    );
    assert!(
        pending == 0,
        "startup must retire the prepared record"
    );
    drop(restarted);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn should_discard_unique_publication_record_when_its_table_no_longer_exists() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie-unique-publication-orphan-{}",
        uuid::Uuid::new_v4()
    ));
    let cassie =
        Cassie::new_local_with_data_dir_and_config(&path, CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let mut constraint = FieldConstraint::new("value");
    constraint.unique = true;
    cassie
        .midge
        .seed_pending_unique_constraint_publication(
            CANONICAL,
            vec![constraint.clone()],
            vec![constraint],
        )
        .unwrap();
    drop(cassie);

    // Act
    let restarted =
        Cassie::new_local_with_data_dir_and_config(&path, CassieRuntimeConfig::default()).unwrap();
    let started = restarted.startup();
    let pending = restarted
        .midge
        .pending_unique_constraint_publication_count()
        .unwrap();

    // Assert
    assert!(started.is_ok(), "startup must survive an orphaned record");
    assert!(pending == 0, "orphaned record must be discarded");
    drop(restarted);
    std::fs::remove_dir_all(path).unwrap();
}

fn schema() -> Schema {
    Schema {
        fields: vec![FieldSchema {
            name: "value".into(),
            data_type: DataType::Text,
            nullable: true,
        }],
    }
}
