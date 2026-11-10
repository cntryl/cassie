use super::decode_identity;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use std::time::Instant;

use crate::app::{Cassie, CassieSession};
use crate::config::CassieRuntimeConfig;
use crate::midge::adapter::{
    key_encoding, Midge, StatementDataRead, StatementReadScope, StorageFamily,
};
use crate::midge::row_blob::encode_row;

struct ReplacementFixture {
    cassie: Cassie,
    session: CassieSession,
    target: String,
}

impl ReplacementFixture {
    fn new() -> Self {
        let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
            .expect("memory fixture");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE copy_identity_reused (payload TEXT)",
                vec![],
            )
            .expect("published target");
        let target = crate::catalog::canonical_relation_name(
            &cassie.default_database,
            crate::catalog::DEFAULT_SCHEMA,
            "copy_identity_reused",
        );
        cassie
            .midge
            .put_document(
                &target,
                Some("old-row".into()),
                serde_json::json!({"payload":"old"}),
            )
            .expect("old row");
        Self {
            cassie,
            session,
            target,
        }
    }

    fn put_legacy(&self, payload: &str) {
        let schema = self
            .cassie
            .midge
            .row_schema(&self.target)
            .expect("descriptor");
        let bytes = encode_row(&schema, &serde_json::json!({"payload":payload})).expect("row");
        let mut tx = self
            .cassie
            .midge
            .transaction(
                StorageFamily::Data,
                cntryl_midge::TransactionMode::ReadWrite,
            )
            .expect("legacy fixture transaction");
        tx.put(key_encoding::doc_key(&self.target, "legacy"), bytes, None)
            .expect("legacy fixture");
        tx.commit(self.cassie.midge.write_options_sync())
            .expect("legacy fixture commit");
    }

    fn replace(&self) {
        for sql in [
            "BEGIN",
            "DROP TABLE copy_identity_reused",
            "CREATE TABLE copy_identity_reused (payload TEXT)",
            "INSERT INTO copy_identity_reused VALUES ('new')",
        ] {
            self.cassie
                .execute_sql(&self.session, sql, vec![])
                .expect("staged replacement");
        }
    }

    fn payloads(&self) -> Vec<String> {
        let mut rows = self
            .cassie
            .midge
            .scan_documents(&self.target)
            .expect("rows")
            .into_iter()
            .map(|row| row.payload["payload"].as_str().expect("text").to_owned())
            .collect::<Vec<_>>();
        rows.sort();
        rows
    }
}

#[test]
fn should_reclaim_only_retired_rows_after_a_recreated_target_commits() {
    // Arrange
    let fixture = ReplacementFixture::new();
    fixture.put_legacy("legacy-old");
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let old_id = fixture
        .cassie
        .midge
        .row_schema(&fixture.target)
        .expect("old descriptor")
        .relation_id;
    let old_reader = StatementDataRead::capture(
        &fixture.cassie.midge,
        &fixture.cassie.default_database,
        &controls,
    )
    .expect("retained statement reader");
    let old_epoch = fixture.cassie.begin_schema_epoch_guard_for_diagnostics();

    // Act
    fixture.replace();
    {
        let overlay = fixture.session.transaction_storage_overlay();
        let _scope = fixture
            .cassie
            .midge
            .enter_transactional_ddl_scope(&fixture.cassie.default_database, overlay, false)
            .expect("staged scope");
        assert!(
            fixture.payloads().is_empty(),
            "staged tombstones hide legacy rows"
        );
    }
    fixture
        .cassie
        .execute_sql(&fixture.session, "COMMIT", vec![])
        .expect("replacement commit");
    fixture.put_legacy("legacy-new");
    let new_id = fixture
        .cassie
        .midge
        .row_schema(&fixture.target)
        .expect("new descriptor")
        .relation_id;
    let hashes = serde_json::to_value(
        fixture
            .cassie
            .midge
            .projection_hash_summary(&fixture.target)
            .expect("hashes"),
    )
    .expect("hash value");
    let generation = fixture
        .cassie
        .midge
        .collection_generation(&fixture.target)
        .expect("generation");
    drop(old_epoch);
    fixture
        .cassie
        .run_deferred_schema_cleanup_for_diagnostics()
        .expect("retired cleanup");

    // Assert
    assert!(old_id > 0 && new_id > 0);
    assert_ne!(old_id, new_id);
    assert!(fixture
        .cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &Midge::row_prefix(old_id))
        .expect("retired rows")
        .is_empty());
    assert_eq!(fixture.payloads(), ["legacy-new", "new"]);
    assert_eq!(
        fixture
            .cassie
            .midge
            .collection_generation(&fixture.target)
            .expect("generation"),
        generation
    );
    assert_eq!(
        serde_json::to_value(
            fixture
                .cassie
                .midge
                .projection_hash_summary(&fixture.target)
                .expect("hashes")
        )
        .expect("hash value"),
        hashes
    );
    let _scope = StatementReadScope::enter(Some(&old_reader));
    assert_eq!(fixture.payloads(), ["legacy-old", "old"]);
}

#[test]
fn should_reject_replacement_publication_after_a_supported_concurrent_old_target_write() {
    // Arrange
    let fixture = ReplacementFixture::new();
    fixture.replace();
    fixture
        .cassie
        .midge
        .put_document(
            &fixture.target,
            Some("concurrent".into()),
            serde_json::json!({"payload":"concurrent"}),
        )
        .expect("supported concurrent old-target writer");

    // Act
    let committed = fixture
        .cassie
        .execute_sql(&fixture.session, "COMMIT", vec![]);
    fixture
        .cassie
        .execute_sql(&fixture.session, "ROLLBACK", vec![])
        .expect("rollback");

    // Assert
    assert!(
        committed.is_err(),
        "changed generation must reject the staged replacement"
    );
    assert_eq!(fixture.payloads(), ["concurrent", "old"]);
}

#[test]
fn should_fail_closed_for_a_legacy_row_descriptor_without_identity() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    // Act
    let id = decode_identity(br#"{"schema_version":1,"fields":[]}"#, &controls).expect("decode");
    // Assert
    assert_eq!(id, None);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_deny_identity_decode_over_query_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 1,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    // Act
    let id = decode_identity(br#"{"relation_id":42}"#, &controls);
    // Assert
    assert!(id.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
