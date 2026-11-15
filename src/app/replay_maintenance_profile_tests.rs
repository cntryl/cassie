//! Derived state after replay under the strict and replayable profiles (#1277).
//!
//! The eager hash rebuild buffers its intermediate commits. These tests pin
//! what a reader and a later repair observe for each index lifecycle and for an
//! interrupted rebuild, under both replay profiles.

use crate::app::{Cassie, CassieSession, ProjectionReplayBatch, ProjectionReplayEvent};
use crate::config::CassieRuntimeConfig;
use crate::midge::adapter::set_projection_hash_maintenance_failure_point;
use crate::types::Value;

#[derive(Clone, Copy, Debug)]
enum Profile {
    Strict,
    Replayable,
}

const PROFILES: [Profile; 2] = [Profile::Strict, Profile::Replayable];

struct Fixture {
    cassie: Cassie,
    session: CassieSession,
    projection: String,
    path: Option<std::path::PathBuf>,
}

impl Fixture {
    fn in_memory() -> Self {
        let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
        Self::started(cassie, None)
    }

    fn local(path: std::path::PathBuf) -> Self {
        let cassie =
            Cassie::new_local_with_data_dir_and_config(&path, CassieRuntimeConfig::default())
                .unwrap();
        Self::started(cassie, Some(path))
    }

    fn started(cassie: Cassie, path: Option<std::path::PathBuf>) -> Self {
        cassie.startup().unwrap();
        let session = cassie.create_session("projector", None);
        let projection = cassie.midge.canonical_collection_name("maintained_docs");
        Self {
            cassie,
            session,
            projection,
            path,
        }
    }

    fn run(&self, sql: &str) {
        self.cassie.execute_sql(&self.session, sql, vec![]).unwrap();
    }

    fn titles(&self, predicate: &str) -> Vec<Vec<Value>> {
        self.cassie
            .execute_sql(
                &self.session,
                &format!("SELECT title FROM maintained_docs WHERE {predicate} ORDER BY title"),
                vec![],
            )
            .unwrap()
            .rows()
            .to_vec()
    }

    fn replay(&self, profile: Profile, position: u64, title: &str) {
        let batch = ProjectionReplayBatch {
            projection: self.projection.clone(),
            source_identity: "orders".to_owned(),
            batch_id: format!("batch-{position}"),
            lag: 0,
            events: vec![ProjectionReplayEvent {
                event_id: format!("event-{position}"),
                checkpoint: format!("checkpoint-{position}"),
                position: Some(position),
                document_id: format!("doc-{position}"),
                payload: Some(serde_json::json!({"title": title})),
            }],
        };
        let report = match profile {
            Profile::Strict => self.cassie.replay_projection_batch_strict(batch),
            Profile::Replayable => self.cassie.replay_projection_batch(batch),
        }
        .unwrap();
        assert_eq!(report.applied_event_count, 1);
    }

    /// Whether the published hash root covers `rows` rows with no hash debt.
    fn hashes_current(&self, rows: u64) -> bool {
        let root = self.cassie.midge.root_hash(&self.projection).unwrap();
        let debt = self
            .cassie
            .midge
            .has_projection_hash_maintenance_debt(&self.projection)
            .unwrap();
        root.is_some_and(|root| root.row_count == rows) && !debt
    }
}

fn one(title: &str) -> Vec<Vec<Value>> {
    vec![vec![Value::String(title.to_owned())]]
}

#[test]
fn should_publish_current_hashes_for_an_unindexed_projection() {
    for profile in PROFILES {
        // Arrange
        let fixture = Fixture::in_memory();
        fixture.run("CREATE TABLE maintained_docs (title TEXT)");

        // Act
        fixture.replay(profile, 1, "alpha");
        fixture.replay(profile, 2, "beta");

        // Assert
        assert!(fixture.hashes_current(2), "{profile:?}");
        assert_eq!(fixture.titles("title = 'beta'"), one("beta"), "{profile:?}");
    }
}

#[test]
fn should_serve_replayed_rows_through_an_existing_index() {
    for profile in PROFILES {
        // Arrange
        let fixture = Fixture::in_memory();
        fixture.run("CREATE TABLE maintained_docs (title TEXT)");
        fixture.run("CREATE INDEX maintained_title_idx ON maintained_docs USING btree (title)");

        // Act
        fixture.replay(profile, 1, "alpha");
        fixture.replay(profile, 2, "beta");

        // Assert
        assert!(fixture.hashes_current(2), "{profile:?}");
        assert_eq!(
            fixture.titles("title = 'alpha'"),
            one("alpha"),
            "{profile:?}"
        );
    }
}

#[test]
fn should_serve_replayed_rows_through_an_index_created_afterwards() {
    for profile in PROFILES {
        // Arrange
        let fixture = Fixture::in_memory();
        fixture.run("CREATE TABLE maintained_docs (title TEXT)");
        fixture.replay(profile, 1, "alpha");

        // Act
        fixture.run("CREATE INDEX maintained_title_idx ON maintained_docs USING btree (title)");
        fixture.replay(profile, 2, "beta");

        // Assert
        assert!(fixture.hashes_current(2), "{profile:?}");
        assert_eq!(
            fixture.titles("title = 'alpha'"),
            one("alpha"),
            "{profile:?}"
        );
        assert_eq!(fixture.titles("title = 'beta'"), one("beta"), "{profile:?}");
    }
}

#[test]
fn should_serve_replayed_rows_after_the_index_is_dropped() {
    for profile in PROFILES {
        // Arrange
        let fixture = Fixture::in_memory();
        fixture.run("CREATE TABLE maintained_docs (title TEXT)");
        fixture.run("CREATE INDEX maintained_title_idx ON maintained_docs USING btree (title)");
        fixture.replay(profile, 1, "alpha");

        // Act
        fixture.run("DROP INDEX maintained_title_idx");
        fixture.replay(profile, 2, "beta");

        // Assert
        assert!(fixture.hashes_current(2), "{profile:?}");
        assert_eq!(fixture.titles("title = 'beta'"), one("beta"), "{profile:?}");
    }
}

#[test]
fn should_keep_debt_then_repair_after_an_interrupted_hash_refresh() {
    for profile in PROFILES {
        // Arrange
        let fixture = Fixture::in_memory();
        fixture.run("CREATE TABLE maintained_docs (title TEXT)");
        fixture.replay(profile, 1, "alpha");
        set_projection_hash_maintenance_failure_point(true);

        // Act
        fixture.replay(profile, 2, "beta");
        let current_after_failure = fixture.hashes_current(2);
        let rows_after_failure = fixture.titles("title = 'beta'");
        fixture.cassie.midge.retry_maintenance_debt().unwrap();

        // Assert
        assert!(!current_after_failure, "{profile:?}");
        assert_eq!(rows_after_failure, one("beta"), "{profile:?}");
        assert!(fixture.hashes_current(2), "{profile:?}");
    }
}

#[test]
fn should_repair_an_interrupted_hash_refresh_after_restart() {
    for profile in PROFILES {
        // Arrange
        let path = std::env::temp_dir().join(format!(
            "cassie-replay-maintenance-{}",
            uuid::Uuid::new_v4()
        ));
        let fixture = Fixture::local(path.clone());
        fixture.run("CREATE TABLE maintained_docs (title TEXT)");
        fixture.replay(profile, 1, "alpha");
        set_projection_hash_maintenance_failure_point(true);
        fixture.replay(profile, 2, "beta");
        let current_before_restart = fixture.hashes_current(2);
        fixture.cassie.shutdown();
        drop(fixture);

        // Act
        let recovered = Fixture::local(path);
        let rows = recovered.titles("title = 'beta'");
        let current = recovered.hashes_current(2);
        recovered.cassie.shutdown();
        let path = recovered.path.clone().unwrap();
        drop(recovered);
        std::fs::remove_dir_all(path).unwrap();

        // Assert
        assert!(!current_before_restart, "{profile:?}");
        assert_eq!(rows, one("beta"), "{profile:?}");
        assert!(current, "{profile:?}");
    }
}
