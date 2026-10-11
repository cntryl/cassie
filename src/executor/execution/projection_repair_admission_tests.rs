//! Required repair output is admitted before changing persisted repair state.
use std::path::PathBuf;
use std::time::Instant;

use super::*;
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::midge::adapter::{RowHashRecord, StorageFamily, StoredHashState};

struct Directory(PathBuf);

impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("strict repair-admission cleanup");
    }
}

fn corrupt_first_row_hash(cassie: &Cassie, collection: &str) {
    let mut row_hash = cassie
        .midge
        .list_row_hashes(collection)
        .expect("row hashes")[0]
        .clone();
    let key = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("stored hash entries")
        .into_iter()
        .find_map(|(key, value)| {
            let record = serde_json::from_slice::<RowHashRecord>(&value).ok()?;
            (record.collection == collection && record.row_id == row_hash.row_id).then_some(key)
        })
        .expect("exact persisted row-hash key");
    row_hash.state = StoredHashState::Stale;
    let mut transaction = cassie
        .midge
        .data_tx(cntryl_midge::TransactionMode::ReadWrite)
        .expect("corruption transaction");
    transaction
        .put(
            key,
            serde_json::to_vec(&row_hash).expect("row hash encoding"),
            None,
        )
        .expect("write stale hash");
    transaction
        .commit(cassie.midge.write_options_sync())
        .expect("durable stale hash");
}

#[test]
fn should_admit_repair_output_before_rebuilding_persisted_hashes() {
    // Arrange
    let directory = Directory(
        std::env::temp_dir().join(format!("cassie-repair-output-{}", uuid::Uuid::new_v4())),
    );
    std::fs::create_dir_all(&directory.0).expect("owned fixture directory");
    let cassie = Cassie::new_with_data_dir(&directory.0).expect("isolated local store");
    cassie.startup().expect("startup");
    let session = cassie.create_session("root", None);
    for sql in [
        "CREATE TABLE repair_admission_docs(title TEXT)",
        "INSERT INTO repair_admission_docs VALUES ('alpha')",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("valid fixture SQL");
    }
    let collection = cassie
        .catalog
        .get_schema("repair_admission_docs")
        .expect("created collection")
        .collection;
    corrupt_first_row_hash(&cassie, &collection);
    cassie
        .execute_sql(
            &session,
            "VERIFY PROJECTION repair_admission_docs MODE hashes_only",
            vec![],
        )
        .expect("prior repairable verification");
    let before = projection_metadata(&cassie, &collection).expect("verified metadata");
    assert!(before.integrity.repairable);
    assert_eq!(
        before.integrity.state,
        catalog::ProjectionVerificationState::Failed
    );
    let before_integrity = serde_json::to_value(&before.integrity).expect("integrity snapshot");
    assert!(cassie
        .midge
        .list_projection_repair_reports()
        .expect("stored reports")
        .is_empty());
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );

    // Act
    let result = repair_projection(
        &cassie,
        &RepairProjectionStatement {
            target: ProjectionDiffTarget {
                name: collection.clone(),
                version_id: None,
            },
            scope: ProjectionRepairScope::Row,
        },
        &controls,
    );
    let hash_state = cassie
        .midge
        .list_row_hashes(&collection)
        .expect("hash readback")[0]
        .state
        .clone();
    let integrity = serde_json::to_value(
        &projection_metadata(&cassie, &collection)
            .expect("catalog readback")
            .integrity,
    )
    .expect("integrity readback");
    let stored_reports = cassie
        .midge
        .list_projection_repair_reports()
        .expect("stored report readback");
    let catalog_reports = cassie.catalog.list_projection_repair_reports();
    drop((session, cassie, directory));

    // Assert
    let Err(QueryError::Cassie(error @ CassieError::ResourceLimit(_))) = result else {
        panic!("required repair output must return structured memory denial");
    };
    assert_eq!(error.descriptor().sql_state, "54000");
    assert_eq!(
        hash_state,
        StoredHashState::Stale,
        "denied output must precede hash repair"
    );
    assert_eq!(integrity, before_integrity);
    assert!(stored_reports.is_empty());
    assert!(catalog_reports.is_empty());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
