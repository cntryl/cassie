//! Local persistence witness for a replay spanning retained Data snapshots.

use super::{
    controls, replay_snapshot_batch, replay_snapshot_has_event, replay_snapshot_index_ids, Arc,
    Cassie, CassieRuntimeConfig, QueryExecutionControls, StatementDataRead, StatementReadScope,
};
use crate::catalog::IndexMeta;

struct ReplayFixture {
    cassie: Arc<Cassie>,
    projection: String,
    index: IndexMeta,
    controls: QueryExecutionControls,
}

impl ReplayFixture {
    fn new(cassie: Cassie) -> Self {
        let projection = cassie
            .midge
            .canonical_collection_name("replay_durable_snapshot");
        let index = cassie
            .midge
            .get_index(&projection, "replay_durable_value_idx")
            .unwrap()
            .unwrap();
        Self {
            cassie: Arc::new(cassie),
            projection,
            index,
            controls: controls(16 * 1024 * 1024),
        }
    }

    fn capture(&self) -> Arc<StatementDataRead> {
        StatementDataRead::capture(&self.cassie.midge, "postgres", &self.controls).unwrap()
    }

    fn assert_snapshot(&self, owner: &Arc<StatementDataRead>, position: u64) {
        let checkpoint = owner
            .projection_metadata_for_test(&self.projection)
            .unwrap()
            .unwrap()
            .source_checkpoint;
        let row = {
            let _scope = StatementReadScope::enter(Some(owner));
            self.cassie
                .midge
                .get_document(&self.projection, "doc")
                .unwrap()
                .unwrap()
                .payload
        };
        let first = replay_snapshot_has_event(owner, &self.projection, "event-1");
        let second = replay_snapshot_has_event(owner, &self.projection, "event-2");
        let old_index =
            replay_snapshot_index_ids(&self.cassie, &self.index, &self.controls, owner, 1);
        let new_index =
            replay_snapshot_index_ids(&self.cassie, &self.index, &self.controls, owner, 2);

        assert_eq!(
            checkpoint.as_deref(),
            Some(format!("checkpoint-{position}").as_str())
        );
        assert_eq!(row, serde_json::json!({"value": position}));
        assert!(first);
        assert_eq!(second, position == 2);
        if position == 1 {
            assert_eq!(old_index, vec!["doc".to_owned()]);
            assert_eq!(new_index, Vec::<String>::new());
        } else {
            assert_eq!(old_index, Vec::<String>::new());
            assert_eq!(new_index, vec!["doc".to_owned()]);
        }
    }
}

#[test]
fn should_preserve_atomic_replay_in_durable_snapshots_after_restart() {
    // Arrange
    let path =
        std::env::temp_dir().join(format!("cassie-replay-snapshot-{}", uuid::Uuid::new_v4()));
    let cassie =
        Cassie::new_local_with_data_dir_and_config(&path, CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE replay_durable_snapshot (value INT)",
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX replay_durable_value_idx ON replay_durable_snapshot USING btree (value)",
            vec![],
        )
        .unwrap();
    let fixture = ReplayFixture::new(cassie);
    fixture
        .cassie
        .replay_projection_batch_strict(replay_snapshot_batch(&fixture.projection, 1, 1))
        .unwrap();
    let retained = fixture.capture();
    let next = replay_snapshot_batch(&fixture.projection, 2, 2);
    let writer = Arc::clone(&fixture.cassie);

    // Act
    std::thread::spawn(move || writer.replay_projection_batch_strict(next))
        .join()
        .unwrap()
        .unwrap();
    let fresh = fixture.capture();

    // Assert
    fixture.assert_snapshot(&retained, 1);
    fixture.assert_snapshot(&fresh, 2);
    drop(retained);
    drop(fresh);
    fixture.cassie.shutdown();
    drop(fixture);
    drop(session);
    let recovered =
        Cassie::new_local_with_data_dir_and_config(&path, CassieRuntimeConfig::default()).unwrap();
    recovered.startup().unwrap();
    let fixture = ReplayFixture::new(recovered);
    let recovered_snapshot = fixture.capture();
    fixture.assert_snapshot(&recovered_snapshot, 2);
    drop(recovered_snapshot);
    let retry = fixture
        .cassie
        .replay_projection_batch_strict(replay_snapshot_batch(&fixture.projection, 2, 2))
        .unwrap();
    assert_eq!(retry.applied_event_count, 0);
    assert_eq!(retry.skipped_duplicate_count, 1);
    let after_retry = fixture.capture();
    fixture.assert_snapshot(&after_retry, 2);
    drop(after_retry);
    fixture.cassie.shutdown();
    drop(fixture);
    std::fs::remove_dir_all(path).unwrap();
}
