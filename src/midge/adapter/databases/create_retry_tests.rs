use super::{
    key_encoding, DatabaseLifecycleKind, DatabaseLifecycleRecord, DatabaseMeta, Midge, Uuid,
};
use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use crate::types::Value;
use std::path::Path;

const DATABASE: &str = "retry_db";

fn open(path: &Path) -> Cassie {
    Cassie::new_local_with_data_dir_and_config(path, CassieRuntimeConfig::default())
        .expect("open local store")
}

fn retry_create(cassie: &Cassie) -> (DatabaseLifecycleRecord, DatabaseMeta) {
    let abandoned = DatabaseMeta::new(DATABASE, None);
    let intent = DatabaseLifecycleRecord {
        operation_id: Uuid::new_v4().to_string(),
        kind: DatabaseLifecycleKind::Create,
        name: abandoned.name,
        physical_family: abandoned.physical_family,
    };
    let mut tx = cassie.midge.begin_schema_rw_tx().unwrap();
    Midge::write_lifecycle_record(&mut tx, &intent).unwrap();
    tx.commit(cassie.midge.write_options_sync()).unwrap();

    let root = cassie.create_session("root", None);
    cassie
        .execute_sql(&root, "CREATE DATABASE retry_db", vec![])
        .expect("retry database creation");
    let session = cassie.create_session("root", Some(DATABASE.to_owned()));
    for sql in [
        "CREATE TABLE sentinel (id BIGINT)",
        "INSERT INTO sentinel (id) VALUES (731)",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect(sql);
    }
    let replacement = cassie.midge.get_database(DATABASE).unwrap().unwrap();
    assert_ne!(replacement.physical_family, intent.physical_family);
    assert_owner_and_row(cassie, &replacement);
    let families = cassie.midge.engine.list_column_families().unwrap();
    assert!(!families.iter().any(|f| f.name() == intent.physical_family));
    assert!(families
        .iter()
        .any(|f| f.name() == replacement.physical_family));
    let key = key_encoding::database_lifecycle_key(&intent.operation_id);
    let read = cassie.midge.begin_schema_readonly_tx().unwrap();
    assert_eq!(
        read.get(&key).unwrap().as_deref(),
        Some(serde_json::to_vec(&intent).unwrap().as_slice())
    );
    (intent, replacement)
}

fn assert_owner_and_row(cassie: &Cassie, replacement: &DatabaseMeta) {
    assert_eq!(
        cassie.midge.get_database(DATABASE).unwrap().as_ref(),
        Some(replacement)
    );
    assert_eq!(
        cassie.midge.database_family(DATABASE).unwrap().name(),
        replacement.physical_family
    );
    let session = cassie.create_session("root", Some(DATABASE.to_owned()));
    let result = cassie
        .execute_sql(&session, "SELECT id FROM sentinel", vec![])
        .unwrap();
    assert_eq!(result.rows(), &[vec![Value::Int64(731)]]);
}

#[test]
fn should_preserve_published_database_after_interrupted_create_retry_on_restart() {
    // Arrange
    let path = std::env::temp_dir().join(format!("cassie-create-retry-{}", Uuid::new_v4()));
    let cassie = open(&path);
    cassie.startup().expect("initialize local store");
    let (intent, replacement) = retry_create(&cassie);
    drop(cassie);

    // Act
    let restarted = open(&path);
    restarted
        .startup()
        .expect("recover the published replacement");

    // Assert
    assert_owner_and_row(&restarted, &replacement);
    let key = key_encoding::database_lifecycle_key(&intent.operation_id);
    assert!(restarted
        .midge
        .begin_schema_readonly_tx()
        .unwrap()
        .get(&key)
        .unwrap()
        .is_none());
    drop(restarted);
    let reopened = open(&path);
    reopened.startup().expect("second cold startup");
    assert_owner_and_row(&reopened, &replacement);
    drop(reopened);
    std::fs::remove_dir_all(path).unwrap();
}
