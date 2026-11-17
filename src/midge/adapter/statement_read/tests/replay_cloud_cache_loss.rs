//! Killed-process recovery after removing only the simulated cloud's local cache.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::app::{Cassie, CassieError, ProjectionReplayBatch, ProjectionReplayEvent};
use crate::config::CassieRuntimeConfig;
use crate::midge::adapter::{Midge, ScalarIndexScanRequest};
use crate::types::Value;

use super::replay_process_crash::{kill_test_child, wait_after_marker, OwnedDataDir};

const CHILD_PROFILE: &str = "CASSIE_TEST_CLOUD_CACHE_LOSS_PROFILE";
const CHILD_CACHE: &str = "CASSIE_TEST_CLOUD_CACHE_LOSS_PATH";
const CHILD_TOKEN: &str = "CASSIE_TEST_CLOUD_CACHE_LOSS_TOKEN";
const OWNER_FILE: &str = "cache-loss-owner";
const SOURCE_FILE: &str = "retained-source.json";
const TABLE: &str = "cloud_cache_docs";
const INDEX: &str = "cloud_cache_title_idx";
const LEASE_TTL: Duration = Duration::from_secs(60);
const RECOVERY_DEADLINE: Duration = Duration::from_secs(120);
const LEASE_RECHECK: Duration = Duration::from_millis(250);
type TreeSnapshot = BTreeMap<PathBuf, Option<[u8; 32]>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Profile {
    ApiStrict,
    ApiReplayable,
    SqlStrict,
    SqlReplayable,
}

impl Profile {
    const fn label(self) -> &'static str {
        match self {
            Self::ApiStrict => "api_strict",
            Self::ApiReplayable => "api_replayable",
            Self::SqlStrict => "sql_strict",
            Self::SqlReplayable => "sql_replayable",
        }
    }

    const fn test_name(self) -> &'static str {
        match self {
            Self::ApiStrict => "midge::adapter::statement_read::tests::replay_cloud_cache_loss::should_recover_strict_api_acknowledgement_after_simulated_cloud_cache_loss",
            Self::ApiReplayable => "midge::adapter::statement_read::tests::replay_cloud_cache_loss::should_reconcile_api_prefix_after_simulated_cloud_cache_loss",
            Self::SqlStrict => "midge::adapter::statement_read::tests::replay_cloud_cache_loss::should_recover_strict_sql_acknowledgement_after_simulated_cloud_cache_loss",
            Self::SqlReplayable => "midge::adapter::statement_read::tests::replay_cloud_cache_loss::should_reconcile_sql_prefix_after_simulated_cloud_cache_loss",
        }
    }

    const fn is_api(self) -> bool {
        matches!(self, Self::ApiStrict | Self::ApiReplayable)
    }

    const fn is_strict(self) -> bool {
        matches!(self, Self::ApiStrict | Self::SqlStrict)
    }
}

#[test]
fn should_recover_strict_api_acknowledgement_after_simulated_cloud_cache_loss() {
    // Arrange
    let profile = Profile::ApiStrict;

    // Act
    let recovered = run_cache_loss(profile);

    // Assert
    assert_eq!(recovered, 2);
}

#[test]
fn should_reconcile_api_prefix_after_simulated_cloud_cache_loss() {
    // Arrange
    let profile = Profile::ApiReplayable;

    // Act
    let recovered = run_cache_loss(profile);

    // Assert
    assert!(matches!(recovered, 1 | 2));
}

#[test]
fn should_recover_strict_sql_acknowledgement_after_simulated_cloud_cache_loss() {
    // Arrange
    let profile = Profile::SqlStrict;

    // Act
    let recovered = run_cache_loss(profile);

    // Assert
    assert_eq!(recovered, 2);
}

#[test]
fn should_reconcile_sql_prefix_after_simulated_cloud_cache_loss() {
    // Arrange
    let profile = Profile::SqlReplayable;

    // Act
    let recovered = run_cache_loss(profile);

    // Assert
    assert!(matches!(recovered, 1 | 2));
}

#[test]
fn should_refuse_foreign_and_unowned_cache_roots_before_deletion() {
    // Arrange
    let owned = CacheFixture::new();
    let foreign = CacheFixture::new();
    seed_cache_sentinels(&owned);
    seed_cache_sentinels(&foreign);
    let foreign_before = tree_snapshot(&foreign.root).unwrap();

    // Act
    let foreign_error = wipe_local_cache(&owned, &foreign.cache()).unwrap_err();
    std::fs::write(owned.root.join(OWNER_FILE), "another-owner").unwrap();
    let unowned_before = tree_snapshot(&owned.root).unwrap();
    let unowned_error = wipe_local_cache(&owned, &owned.cache()).unwrap_err();

    // Assert
    assert_eq!(foreign_error.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(unowned_error.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(tree_snapshot(&foreign.root).unwrap(), foreign_before);
    assert_eq!(tree_snapshot(&owned.root).unwrap(), unowned_before);
}

#[cfg(unix)]
#[test]
fn should_refuse_symlinked_cache_before_deletion() {
    // Arrange
    let owned = CacheFixture::new();
    let outside = CacheFixture::new();
    seed_cache_sentinels(&outside);
    let outside_before = tree_snapshot(&outside.root).unwrap();
    std::fs::remove_dir(owned.cache()).unwrap();
    std::os::unix::fs::symlink(outside.cache(), owned.cache()).unwrap();

    // Act
    let error = wipe_local_cache(&owned, &owned.cache()).unwrap_err();

    // Assert
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(tree_snapshot(&outside.root).unwrap(), outside_before);
}

#[test]
fn should_preserve_remote_hashes_and_retained_source_when_wiping_owned_cache() {
    // Arrange
    let owned = CacheFixture::new();
    seed_cache_sentinels(&owned);
    let remote_before = tree_snapshot(&owned.cache().join("cloud_store")).unwrap();
    let source_before = std::fs::read(owned.root.join(SOURCE_FILE)).unwrap();

    // Act
    wipe_local_cache(&owned, &owned.cache()).unwrap();

    // Assert
    assert_eq!(
        tree_snapshot(&owned.cache().join("cloud_store")).unwrap(),
        remote_before
    );
    assert_eq!(
        std::fs::read(owned.root.join(SOURCE_FILE)).unwrap(),
        source_before
    );
    assert_eq!(std::fs::read_dir(owned.cache()).unwrap().count(), 1);
    assert!(!owned.cache().join("wal").exists());
}

#[test]
fn should_refuse_owned_baseline_shutdown_while_an_adapter_owner_is_retained() {
    // Arrange
    let midge = Arc::new(Midge::new_in_memory("postgres").unwrap());
    let retained = Arc::clone(&midge);

    // Act
    let result = midge.shutdown_owned_for_test(Duration::from_secs(30));

    // Assert
    assert!(matches!(result, Err(CassieError::Execution(message))
        if message.starts_with("owned fixture shutdown requires exclusive adapter ownership")));
    assert_eq!(Arc::strong_count(&retained), 1);
    retained.shutdown_owned_for_test(Duration::from_secs(30)).unwrap();
}

struct CacheFixture {
    _directory: OwnedDataDir,
    root: PathBuf,
    token: String,
}

impl CacheFixture {
    fn new() -> Self {
        let directory = OwnedDataDir::new();
        let root = directory.0.canonicalize().unwrap();
        let token = uuid::Uuid::new_v4().to_string();
        std::fs::write(root.join(OWNER_FILE), &token).unwrap();
        std::fs::write(
            root.join(SOURCE_FILE),
            br#"[{"position":1,"id":1,"title":"before"},{"position":2,"id":2,"title":"after"}]"#,
        )
        .unwrap();
        std::fs::create_dir(root.join("cache")).unwrap();
        Self {
            _directory: directory,
            root,
            token,
        }
    }

    fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }
}

fn denied(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

fn tree_snapshot(root: &Path) -> io::Result<TreeSnapshot> {
    fn visit(root: &Path, path: &Path, snapshot: &mut TreeSnapshot) -> io::Result<()> {
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            return Err(denied("cache-loss fixture refuses symlinks"));
        }
        if path != root {
            let relative = path
                .strip_prefix(root)
                .map_err(io::Error::other)?
                .to_owned();
            let digest = if metadata.is_file() {
                Some(Sha256::digest(std::fs::read(path)?).into())
            } else {
                None
            };
            snapshot.insert(relative, digest);
        }
        if metadata.is_dir() {
            for entry in std::fs::read_dir(path)? {
                visit(root, &entry?.path(), snapshot)?;
            }
        } else if !metadata.is_file() {
            return Err(denied("cache-loss fixture refuses special files"));
        }
        Ok(())
    }
    let mut snapshot = BTreeMap::new();
    visit(root, root, &mut snapshot)?;
    Ok(snapshot)
}

fn wipe_local_cache(owned: &CacheFixture, candidate: &Path) -> io::Result<TreeSnapshot> {
    if candidate != owned.cache()
        || std::fs::symlink_metadata(&owned.root)?
            .file_type()
            .is_symlink()
        || owned.root.canonicalize()? != owned.root
        || std::fs::symlink_metadata(owned.root.join(OWNER_FILE))?
            .file_type()
            .is_symlink()
        || std::fs::read_to_string(owned.root.join(OWNER_FILE))? != owned.token
        || std::fs::symlink_metadata(candidate)?
            .file_type()
            .is_symlink()
        || candidate.canonicalize()? != candidate
    {
        return Err(denied("cache-loss fixture does not own this cache root"));
    }
    // Preflight every entry before deleting anything; refuse nested symlinks too.
    tree_snapshot(candidate)?;
    let remote = candidate.join("cloud_store");
    if !std::fs::symlink_metadata(&remote)?.is_dir() {
        return Err(denied("simulated remote store must be a real directory"));
    }
    let before = tree_snapshot(&remote)?;
    if !before.values().any(Option::is_some) {
        return Err(denied("simulated remote store must contain retained files"));
    }
    let mut removed = 0;
    for entry in std::fs::read_dir(candidate)? {
        let entry = entry?;
        if entry.file_name() == OsStr::new("cloud_store") {
            continue;
        }
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(entry.path())?;
        } else {
            std::fs::remove_file(entry.path())?;
        }
        removed += 1;
    }
    if removed == 0 {
        return Err(io::Error::other("fixture had no local cache to lose"));
    }
    if tree_snapshot(&remote)? != before {
        return Err(io::Error::other(
            "simulated remote contents changed during cache deletion",
        ));
    }
    Ok(before)
}

fn seed_cache_sentinels(owned: &CacheFixture) {
    std::fs::create_dir_all(owned.cache().join("cloud_store/wal")).unwrap();
    std::fs::write(owned.cache().join("cloud_store/wal/retained"), "remote").unwrap();
    std::fs::create_dir(owned.cache().join("wal")).unwrap();
    std::fs::write(owned.cache().join("wal/local"), "local").unwrap();
}

fn open_cloud(cache: &Path, configured_strict: bool) -> Result<Cassie, CassieError> {
    let config = CassieRuntimeConfig::default();
    let midge = Midge::new_cloud_simulated_with_lease_for_test(
        cache,
        &config.database,
        configured_strict,
        LEASE_TTL,
    )?;
    Cassie::new_with_midge_and_config(Arc::new(midge), config)
}

fn start_cloud(cache: &Path, configured_strict: bool) -> Cassie {
    let cassie = open_cloud(cache, configured_strict).expect("open canonical simulated cloud");
    cassie.startup().expect("start simulated cloud Cassie");
    cassie
}

fn recover_cloud(cache: &Path) -> Cassie {
    let deadline = Instant::now() + RECOVERY_DEADLINE;
    loop {
        match open_cloud(cache, false) {
            Ok(cassie) => {
                cassie.startup().expect("hydrate recovered cloud catalog");
                return cassie;
            }
            Err(CassieError::Storage(message)) if message.starts_with("Writer lease held: ") => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                assert!(
                    !remaining.is_zero(),
                    "simulated cloud writer lease did not expire"
                );
                std::thread::park_timeout(LEASE_RECHECK.min(remaining));
            }
            Err(error) => panic!("unexpected simulated cloud recovery error: {error}"),
        }
    }
}

fn event(position: u64) -> ProjectionReplayBatch {
    ProjectionReplayBatch {
        projection: TABLE.to_owned(),
        source_identity: "retained-orders".to_owned(),
        batch_id: format!("batch-{position}"),
        lag: 0,
        events: vec![ProjectionReplayEvent {
            event_id: format!("event-{position}"),
            checkpoint: format!("checkpoint-{position}"),
            position: Some(position),
            document_id: format!("doc-{position}"),
            payload: Some(serde_json::json!({
                "id": position,
                "title": if position == 1 { "before" } else { "after" }
            })),
        }],
    }
}

fn apply_event(cassie: &Cassie, profile: Profile, position: u64) {
    if profile.is_api() {
        let report = if profile.is_strict() {
            cassie.replay_projection_batch_strict(event(position))
        } else {
            cassie.replay_projection_batch(event(position))
        }
        .expect("acknowledge real replay batch");
        assert_eq!(report.applied_event_count, 1);
    } else {
        let session = cassie.create_session("projector", None);
        let mode = if profile.is_strict() {
            "strict"
        } else {
            "replayable"
        };
        let title = if position == 1 { "before" } else { "after" };
        for sql in [
            "BEGIN".to_owned(),
            format!("SET LOCAL cassie.projection_write_mode = '{mode}'"),
            format!("INSERT INTO {TABLE} (id, title) VALUES ({position}, '{title}')"),
            format!("UPDATE cloud_cache_checkpoint SET position = {position} WHERE id = 1"),
            "COMMIT".to_owned(),
        ] {
            cassie
                .execute_sql(&session, &sql, vec![])
                .expect("acknowledge SQL projection transaction");
        }
    }
}

fn baseline(cache: &Path, profile: Profile) {
    let cassie = start_cloud(cache, true);
    let session = cassie.create_session("projector", None);
    for sql in [
        "CREATE TABLE cloud_cache_docs (id BIGINT PRIMARY KEY, title TEXT)",
        "CREATE INDEX cloud_cache_title_idx ON cloud_cache_docs USING btree (title)",
        "CREATE TABLE cloud_cache_checkpoint (id BIGINT PRIMARY KEY, position BIGINT)",
        "INSERT INTO cloud_cache_checkpoint (id, position) VALUES (1, 0)",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("create strictly remote baseline");
    }
    let strict = if profile.is_api() {
        Profile::ApiStrict
    } else {
        Profile::SqlStrict
    };
    apply_event(&cassie, strict, 1);
    assert_state(&cassie, profile, 1);
    cassie.shutdown();
    drop(session);
    drop(cassie);
}

fn recovered_position(cassie: &Cassie, profile: Profile) -> u64 {
    if profile.is_api() {
        cassie
            .midge
            .projection_metadata(TABLE)
            .unwrap()
            .unwrap()
            .source_position
            .unwrap()
    } else {
        let session = cassie.create_session("projector", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT position FROM cloud_cache_checkpoint WHERE id = 1",
                vec![],
            )
            .unwrap();
        assert_eq!(result.rows().len(), 1);
        let Value::Int64(position) = result.rows()[0][0] else {
            panic!("integer checkpoint")
        };
        u64::try_from(position).expect("nonnegative recovered checkpoint")
    }
}

fn assert_state(cassie: &Cassie, profile: Profile, position: u64) {
    assert!(matches!(position, 1 | 2));
    assert_eq!(recovered_position(cassie, profile), position);
    let session = cassie.create_session("projector", None);
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT id, title FROM cloud_cache_docs ORDER BY id",
            vec![],
        )
        .unwrap();
    let mut expected = vec![vec![Value::Int64(1), Value::String("before".to_owned())]];
    if position == 2 {
        expected.push(vec![Value::Int64(2), Value::String("after".to_owned())]);
    }
    assert_eq!(
        rows.rows(),
        expected.as_slice(),
        "rows must match checkpoint before replay"
    );
    let index = cassie
        .midge
        .get_index(TABLE, INDEX)
        .unwrap()
        .expect("retained index metadata");
    for (title, expected_count) in [("before", 1), ("after", usize::from(position == 2))] {
        let controls = cassie.runtime.query_controls(Instant::now());
        let hits = cassie
            .midge
            .scan_scalar_index_controlled(
                &index,
                &ScalarIndexScanRequest {
                    equality_prefix: vec![serde_json::json!(title)],
                    ..Default::default()
                },
                &controls,
            )
            .unwrap();
        assert_eq!(hits.as_slice().len(), expected_count);
        for hit in hits.as_slice() {
            let row = cassie
                .midge
                .get_document(TABLE, &hit.id)
                .unwrap()
                .expect("indexed row");
            assert_eq!(row.payload["title"], title);
        }
    }
    if profile.is_api() {
        let metadata = cassie.midge.projection_metadata(TABLE).unwrap().unwrap();
        assert_eq!(
            metadata.source_checkpoint,
            Some(format!("checkpoint-{position}"))
        );
        assert_eq!(
            metadata.last_applied_event_id,
            Some(format!("event-{position}"))
        );
        for event_position in 1..=2 {
            assert_eq!(
                cassie
                    .midge
                    .has_projection_event(
                        TABLE,
                        "retained-orders",
                        &format!("event-{event_position}")
                    )
                    .unwrap(),
                event_position <= position
            );
        }
    }
}

fn child_main(cache: &Path, profile: Profile) -> ! {
    let cassie = start_cloud(cache, false);
    apply_event(&cassie, profile, 2);
    let token = std::env::var(CHILD_TOKEN).expect("child marker token");
    wait_after_marker(&format!(
        "CASSIE_CLOUD_CACHE_LOSS|{}|{token}|ACK",
        profile.label()
    ))
}

fn reconcile(cassie: &Cassie, profile: Profile, recovered: u64) {
    if profile.is_api() {
        let report = if profile.is_strict() {
            cassie.replay_projection_batch_strict(event(2))
        } else {
            cassie.replay_projection_batch(event(2))
        }
        .expect("rewind and replay retained event");
        assert_eq!(report.applied_event_count, u64::from(recovered == 1));
        assert_eq!(report.skipped_duplicate_count, u64::from(recovered == 2));
        assert_state(cassie, profile, 2);
        let duplicate = cassie.replay_projection_batch(event(2)).unwrap();
        assert_eq!(duplicate.applied_event_count, 0);
        assert_eq!(duplicate.skipped_duplicate_count, 1);
    } else {
        for expected_apply in [recovered == 1, false] {
            let apply = recovered_position(cassie, profile) < 2;
            assert_eq!(
                apply, expected_apply,
                "projector uses recovered checkpoint, not external offset"
            );
            if apply {
                apply_event(cassie, profile, 2);
            }
            assert_state(cassie, profile, 2);
        }
    }
    assert_state(cassie, profile, 2);
}

fn run_cache_loss(profile: Profile) -> u64 {
    if let Some(child_profile) = std::env::var_os(CHILD_PROFILE) {
        assert_eq!(child_profile, OsStr::new(profile.label()));
        let cache = PathBuf::from(std::env::var_os(CHILD_CACHE).expect("child cache path"));
        child_main(&cache, profile);
    }
    let owned = CacheFixture::new();
    baseline(&owned.cache(), profile);
    let source_before = std::fs::read(owned.root.join(SOURCE_FILE)).unwrap();
    let marker = format!(
        "CASSIE_CLOUD_CACHE_LOSS|{}|{}|ACK",
        profile.label(),
        owned.token
    );
    kill_test_child(
        profile.test_name(),
        &[
            (CHILD_PROFILE, OsStr::new(profile.label())),
            (CHILD_CACHE, owned.cache().as_os_str()),
            (CHILD_TOKEN, OsStr::new(&owned.token)),
        ],
        &marker,
    );
    let remote_snapshot = wipe_local_cache(&owned, &owned.cache()).unwrap();
    assert_eq!(
        std::fs::read(owned.root.join(SOURCE_FILE)).unwrap(),
        source_before
    );
    let cassie = recover_cloud(&owned.cache());
    let position = recovered_position(&cassie, profile);
    if profile.is_strict() {
        assert_eq!(position, 2);
    }
    assert_state(&cassie, profile, position);
    reconcile(&cassie, profile, position);
    println!(
        "CASSIE_CLOUD_CACHE_LOSS|{}|recovered_position={position}|retained_remote_entries={}",
        profile.label(),
        remote_snapshot.len()
    );
    cassie.shutdown();
    drop(cassie);
    position
}
