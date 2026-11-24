use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use crate::midge::adapter::{create_retry_test_hooks as hooks, CreateRetryAmbiguity, CreateRetrySeed};
use crate::types::Value;
use std::path::Path;
use uuid::Uuid;

const DATABASE: &str = "retry_db";

fn open(path: &Path) -> Cassie {
    Cassie::new_local_with_data_dir_and_config(path, CassieRuntimeConfig::default())
        .expect("open local store")
}

fn retry_create(cassie: &Cassie) -> (CreateRetrySeed, crate::catalog::DatabaseMeta) {
    let seed = hooks::seed_create_retry(&cassie.midge);
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
    assert_ne!(replacement.physical_family, seed.staged_family);
    assert_owner_and_row(cassie, &replacement);
    let families = cassie.midge.engine.list_column_families().unwrap();
    assert!(!families.iter().any(|family| family.name() == seed.staged_family));
    assert!(families
        .iter()
        .any(|family| family.name() == replacement.physical_family));
    assert!(hooks::has_seeded_create_retry(&cassie.midge, &seed));
    (seed, replacement)
}

fn assert_owner_and_row(cassie: &Cassie, replacement: &crate::catalog::DatabaseMeta) {
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
    let (seed, replacement) = retry_create(&cassie);
    let table = "retry_db.public.sentinel";
    cassie.refresh_cardinality_stats(table).unwrap();
    let stats = cassie.midge.get_cardinality_stats(table).unwrap().unwrap();
    assert_eq!(stats.row_count, 1);
    let expected = hooks::expected_snapshot_after_create_retry(&cassie.midge, &seed);
    drop(cassie);

    // Act
    let restarted = open(&path);
    restarted
        .startup()
        .expect("recover the published replacement");

    // Assert
    assert_eq!(hooks::snapshot(&restarted.midge), expected);
    assert_owner_and_row(&restarted, &replacement);
    drop(restarted);
    let reopened = open(&path);
    reopened.startup().expect("second cold startup");
    assert_owner_and_row(&reopened, &replacement);
    drop(reopened);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn should_preserve_durable_state_when_create_retry_cannot_be_proven() {
    for problem in [
        CreateRetryAmbiguity::StagedFamilyStillPresent,
        CreateRetryAmbiguity::StagedFamilyOwned,
        CreateRetryAmbiguity::ReplacementMissing,
        CreateRetryAmbiguity::ReplacementShared,
        CreateRetryAmbiguity::UnlistedReplacementOwner,
        CreateRetryAmbiguity::ReplacementUnlisted,
        CreateRetryAmbiguity::WrongJournalKey,
        CreateRetryAmbiguity::InvalidStagedFamily,
        CreateRetryAmbiguity::InvalidReplacementFamily,
        CreateRetryAmbiguity::RegistryKeyMismatch,
    ] {
        // Arrange
        let path = std::env::temp_dir().join(format!("cassie-create-retry-{}", Uuid::new_v4()));
        let cassie = open(&path);
        cassie.startup().unwrap();
        let (seed, replacement) = retry_create(&cassie);
        hooks::persist_ambiguity(
            &cassie.midge,
            &seed,
            &replacement.physical_family,
            problem,
        );
        let before = hooks::snapshot(&cassie.midge);
        drop(cassie);

        // Act
        let restarted = open(&path);
        let attempted = restarted.startup();

        // Assert
        assert!(
            matches!(attempted, Err(crate::app::CassieError::StorageBootstrap(_))),
            "{problem:?}: {attempted:?}"
        );
        drop(restarted);
        let retained = open(&path);
        assert_eq!(hooks::snapshot(&retained.midge), before, "{problem:?}");
        drop(retained);
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[test]
fn should_reject_overlapping_journal_ownership_before_either_entry_is_replayed() {
    for sibling_first in [false, true] {
        // Arrange
        let path = std::env::temp_dir().join(format!("cassie-create-retry-{}", Uuid::new_v4()));
        let cassie = open(&path);
        cassie.startup().unwrap();
        let (seed, replacement) = retry_create(&cassie);
        hooks::persist_overlapping_intents(
            &cassie.midge,
            &seed,
            &replacement.physical_family,
            sibling_first,
        );
        let before = hooks::snapshot(&cassie.midge);
        drop(cassie);

        // Act
        let reopened = open(&path);
        let result = reopened.startup();

        // Assert
        assert!(matches!(
            result,
            Err(crate::app::CassieError::StorageBootstrap(_))
        ));
        drop(reopened);
        let retained = open(&path);
        assert_eq!(hooks::snapshot(&retained.midge), before);
        drop(retained);
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[test]
fn should_replay_independent_intents_alongside_a_valid_create_retry() {
    // Arrange
    let path = std::env::temp_dir().join(format!("cassie-create-retry-{}", Uuid::new_v4()));
    let cassie = open(&path);
    cassie.startup().unwrap();
    let (_, replacement) = retry_create(&cassie);
    let orphan_family =
        hooks::persist_independent_intents(&cassie.midge, &replacement.physical_family);
    hooks::create_orphan_family(&cassie.midge, &orphan_family);
    drop(cassie);

    // Act
    let restarted = open(&path);
    restarted.startup().expect("recover the mixed journal");

    // Assert
    assert_owner_and_row(&restarted, &replacement);
    assert!(!restarted
        .midge
        .engine
        .list_column_families()
        .unwrap()
        .iter()
        .any(|family| family.name() == orphan_family));
    assert!(hooks::lifecycle_journal_is_empty(&restarted.midge));
    drop(restarted);
    std::fs::remove_dir_all(path).unwrap();
}
