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
    // Cold catalog hydration also rebuilds stale statistics. Materialize the
    // sentinel's current generation before comparing every persisted key.
    let table = "retry_db.public.sentinel";
    cassie.refresh_cardinality_stats(table).unwrap();
    let stats = cassie.midge.get_cardinality_stats(table).unwrap().unwrap();
    assert_eq!(stats.row_count, 1);
    let key = key_encoding::database_lifecycle_key(&intent.operation_id);
    let mut expected = stored_families(&cassie.midge);
    expected
        .get_mut(super::super::SCHEMA_FAMILY_NAME)
        .unwrap()
        .retain(|(entry, _)| entry != &key);
    drop(cassie);

    // Act
    let restarted = open(&path);
    restarted
        .startup()
        .expect("recover the published replacement");

    // Assert
    assert_eq!(stored_families(&restarted.midge), expected);
    assert_owner_and_row(&restarted, &replacement);
    drop(restarted);
    let reopened = open(&path);
    reopened.startup().expect("second cold startup");
    assert_owner_and_row(&reopened, &replacement);
    drop(reopened);
    std::fs::remove_dir_all(path).unwrap();
}

#[derive(Clone, Copy, Debug)]
enum Ambiguity {
    StagedFamilyStillPresent,
    StagedFamilyOwned,
    ReplacementMissing,
    ReplacementShared,
    UnlistedReplacementOwner,
    ReplacementUnlisted,
    WrongJournalKey,
    InvalidStagedFamily,
    InvalidReplacementFamily,
    RegistryKeyMismatch,
}

impl Ambiguity {
    fn persist(
        self,
        midge: &Midge,
        mut intent: DatabaseLifecycleRecord,
        replacement: &DatabaseMeta,
    ) {
        let mut tx = midge.begin_schema_rw_tx().unwrap();
        match self {
            Self::StagedFamilyStillPresent => {
                midge
                    .create_physical_family(&intent.physical_family)
                    .unwrap();
            }
            Self::StagedFamilyOwned | Self::ReplacementShared | Self::UnlistedReplacementOwner => {
                let family = if matches!(self, Self::StagedFamilyOwned) {
                    &intent.physical_family
                } else {
                    &replacement.physical_family
                };
                let other = DatabaseMeta::with_physical_family("other", None, family);
                if matches!(self, Self::UnlistedReplacementOwner) {
                    tx.put(
                        Midge::database_key(&other.name),
                        serde_json::to_vec(&other).unwrap(),
                        None,
                    )
                    .unwrap();
                } else {
                    Midge::write_database_metadata(&mut tx, &other).unwrap();
                }
            }
            Self::ReplacementMissing => {
                midge
                    .drop_physical_family_if_present(&replacement.physical_family)
                    .unwrap();
            }
            Self::ReplacementUnlisted => {
                let mut names = Midge::load_databases(&tx).unwrap();
                names.retain(|name| name != DATABASE);
                assert_eq!(names, [midge.default_database.as_str()]);
                Midge::save_databases(&mut tx, &names).unwrap();
            }
            Self::WrongJournalKey => {
                Midge::delete_lifecycle_record(&mut tx, &intent.operation_id).unwrap();
                tx.put(
                    key_encoding::database_lifecycle_key(&Uuid::new_v4().to_string()),
                    serde_json::to_vec(&intent).unwrap(),
                    None,
                )
                .unwrap();
            }
            Self::InvalidStagedFamily => {
                intent.physical_family = "db-invalid".to_owned();
                Midge::write_lifecycle_record(&mut tx, &intent).unwrap();
            }
            Self::InvalidReplacementFamily => {
                let meta = DatabaseMeta::with_physical_family(DATABASE, None, "cf0");
                Midge::write_database_metadata(&mut tx, &meta).unwrap();
            }
            Self::RegistryKeyMismatch => {
                let meta =
                    DatabaseMeta::with_physical_family("other", None, &replacement.physical_family);
                tx.put(
                    Midge::database_key(DATABASE),
                    serde_json::to_vec(&meta).unwrap(),
                    None,
                )
                .unwrap();
            }
        }
        tx.commit(midge.write_options_sync()).unwrap();
    }
}

// Read every durable key in this tiny fixture without invoking bootstrap a second time.
fn stored_families(
    midge: &Midge,
) -> std::collections::BTreeMap<String, Vec<super::RawStorageEntry>> {
    midge
        .engine
        .list_column_families()
        .unwrap()
        .into_iter()
        .map(|family| {
            let tx = midge
                .begin_raw_adapter_tx(family.id(), super::TransactionMode::ReadOnly)
                .unwrap();
            let entries = super::collect_scan(tx.scan(&super::Query::new()).unwrap()).unwrap();
            (family.name().to_owned(), entries)
        })
        .collect()
}

#[test]
fn should_preserve_journal_and_families_when_create_retry_cannot_be_proven() {
    for problem in [
        Ambiguity::StagedFamilyStillPresent,
        Ambiguity::StagedFamilyOwned,
        Ambiguity::ReplacementMissing,
        Ambiguity::ReplacementShared,
        Ambiguity::UnlistedReplacementOwner,
        Ambiguity::ReplacementUnlisted,
        Ambiguity::WrongJournalKey,
        Ambiguity::InvalidStagedFamily,
        Ambiguity::InvalidReplacementFamily,
        Ambiguity::RegistryKeyMismatch,
    ] {
        // Arrange
        let path = std::env::temp_dir().join(format!("cassie-create-retry-{}", Uuid::new_v4()));
        let cassie = open(&path);
        cassie.startup().unwrap();
        let (intent, replacement) = retry_create(&cassie);
        problem.persist(&cassie.midge, intent, &replacement);
        let before = stored_families(&cassie.midge);
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
        assert_eq!(stored_families(&retained.midge), before, "{problem:?}");
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
        let (mut intent, replacement) = retry_create(&cassie);
        let first = "00000000-0000-4000-8000-000000000001";
        let last = "ffffffff-ffff-4fff-bfff-fffffffffffe";
        let mut tx = cassie.midge.begin_schema_rw_tx().unwrap();
        Midge::delete_lifecycle_record(&mut tx, &intent.operation_id).unwrap();
        intent.operation_id = if sibling_first { last } else { first }.to_owned();
        let sibling = DatabaseLifecycleRecord {
            operation_id: if sibling_first { first } else { last }.to_owned(),
            kind: if sibling_first {
                DatabaseLifecycleKind::Create
            } else {
                DatabaseLifecycleKind::Drop
            },
            name: "unpublished".to_owned(),
            physical_family: replacement.physical_family,
        };
        Midge::write_lifecycle_record(&mut tx, &intent).unwrap();
        Midge::write_lifecycle_record(&mut tx, &sibling).unwrap();
        tx.commit(cassie.midge.write_options_sync()).unwrap();
        let before = stored_families(&cassie.midge);
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
        assert_eq!(stored_families(&retained.midge), before);
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
    let orphan = DatabaseMeta::new("abandoned", None);
    let mut tx = cassie.midge.begin_schema_rw_tx().unwrap();
    // Exercise the existing raw-metadata fallback for a store without a name list.
    tx.delete(Midge::databases_key()).unwrap();
    for (meta, kind) in [
        (&orphan, DatabaseLifecycleKind::Create),
        (&replacement, DatabaseLifecycleKind::Drop),
    ] {
        let record = DatabaseLifecycleRecord {
            operation_id: Uuid::new_v4().to_string(),
            kind,
            name: meta.name.clone(),
            physical_family: meta.physical_family.clone(),
        };
        Midge::write_lifecycle_record(&mut tx, &record).unwrap();
    }
    tx.commit(cassie.midge.write_options_sync()).unwrap();
    cassie
        .midge
        .create_physical_family(&orphan.physical_family)
        .unwrap();
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
        .any(|f| f.name() == orphan.physical_family));
    let read = restarted.midge.begin_schema_readonly_tx().unwrap();
    let query = super::Query::new().prefix(key_encoding::database_lifecycle_prefix().into());
    let remaining = super::collect_scan(read.scan(&query).unwrap()).unwrap();
    assert_eq!(remaining, [] as [super::RawStorageEntry; 0]);
    drop(read);
    drop(restarted);
    std::fs::remove_dir_all(path).unwrap();
}
