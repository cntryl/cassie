//! Narrow fixture helpers for Engine's database lifecycle restart tests.
use super::{
    DatabaseLifecycleKind, DatabaseLifecycleRecord, DatabaseMeta, Midge, Query, RawStorageEntry,
    TransactionMode,
};
use crate::adapter::{collect_scan, key_encoding};
use std::collections::BTreeMap;
use uuid::Uuid;

const DATABASE: &str = "retry_db";

#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct CreateRetrySeed {
    pub operation_id: String,
    pub staged_family: String,
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub enum CreateRetryAmbiguity {
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

#[doc(hidden)]
#[derive(Debug, PartialEq, Eq)]
pub struct DurableStorageSnapshot(BTreeMap<String, Vec<RawStorageEntry>>);

/// Persists an interrupted create intent with the same journal encoder used by recovery.
#[doc(hidden)]
pub fn seed_create_retry(midge: &Midge) -> CreateRetrySeed {
    let abandoned = DatabaseMeta::new(DATABASE, None);
    let intent = DatabaseLifecycleRecord {
        operation_id: Uuid::new_v4().to_string(),
        kind: DatabaseLifecycleKind::Create,
        name: abandoned.name,
        physical_family: abandoned.physical_family.clone(),
    };
    let mut tx = midge.begin_schema_rw_tx().unwrap();
    Midge::write_lifecycle_record(&mut tx, &intent).unwrap();
    tx.commit(midge.write_options_sync()).unwrap();
    CreateRetrySeed {
        operation_id: intent.operation_id,
        staged_family: abandoned.physical_family,
    }
}

/// Checks the journal using its canonical key and serialized record.
#[doc(hidden)]
pub fn has_seeded_create_retry(midge: &Midge, seed: &CreateRetrySeed) -> bool {
    let key = key_encoding::database_lifecycle_key(&seed.operation_id);
    let Ok(read) = midge.begin_schema_readonly_tx() else {
        return false;
    };
    let Ok(Some(value)) = read.get(&key) else {
        return false;
    };
    let Ok(record) = serde_json::from_slice::<DatabaseLifecycleRecord>(&value) else {
        return false;
    };
    record.operation_id == seed.operation_id
        && record.kind_matches_create()
        && record.name == DATABASE
        && record.physical_family == seed.staged_family
}

/// Captures every key in every physical family for restart-preservation assertions.
#[doc(hidden)]
pub fn snapshot(midge: &Midge) -> DurableStorageSnapshot {
    DurableStorageSnapshot(
        midge
            .engine
            .list_column_families()
            .unwrap()
            .into_iter()
            .map(|family| {
                let tx = midge
                    .begin_raw_adapter_tx(family.id(), TransactionMode::ReadOnly)
                    .unwrap();
                let entries = collect_scan(tx.scan(&Query::new()).unwrap()).unwrap();
                (family.name().to_owned(), entries)
            })
            .collect(),
    )
}

/// Expected full-store image after a cold startup consumes the seeded intent.
#[doc(hidden)]
pub fn expected_snapshot_after_create_retry(
    midge: &Midge,
    seed: &CreateRetrySeed,
) -> DurableStorageSnapshot {
    let mut snapshot = snapshot(midge);
    let lifecycle_key = key_encoding::database_lifecycle_key(&seed.operation_id);
    snapshot
        .0
        .get_mut(crate::adapter::SCHEMA_FAMILY_NAME)
        .unwrap()
        .retain(|(key, _)| key != &lifecycle_key);
    snapshot
}

/// Checks that lifecycle recovery consumed all journal entries.
#[doc(hidden)]
pub fn lifecycle_journal_is_empty(midge: &Midge) -> bool {
    let Ok(read) = midge.begin_schema_readonly_tx() else {
        return false;
    };
    let query = Query::new().prefix(key_encoding::database_lifecycle_prefix().into());
    collect_scan(read.scan(&query).unwrap()).is_ok_and(|entries| entries.is_empty())
}

/// Persists one ambiguous owner/family/journal state for a startup rejection test.
#[doc(hidden)]
pub fn persist_ambiguity(
    midge: &Midge,
    seed: &CreateRetrySeed,
    replacement_family: &str,
    problem: CreateRetryAmbiguity,
) {
    let mut intent = DatabaseLifecycleRecord {
        operation_id: seed.operation_id.clone(),
        kind: DatabaseLifecycleKind::Create,
        name: DATABASE.to_owned(),
        physical_family: seed.staged_family.clone(),
    };
    let mut tx = midge.begin_schema_rw_tx().unwrap();
    match problem {
        CreateRetryAmbiguity::StagedFamilyStillPresent => {
            midge
                .create_physical_family(&intent.physical_family)
                .unwrap();
        }
        CreateRetryAmbiguity::StagedFamilyOwned
        | CreateRetryAmbiguity::ReplacementShared
        | CreateRetryAmbiguity::UnlistedReplacementOwner => {
            let family = if matches!(problem, CreateRetryAmbiguity::StagedFamilyOwned) {
                &intent.physical_family
            } else {
                replacement_family
            };
            let other = DatabaseMeta::with_physical_family("other", None, family);
            if matches!(problem, CreateRetryAmbiguity::UnlistedReplacementOwner) {
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
        CreateRetryAmbiguity::ReplacementMissing => {
            midge
                .drop_physical_family_if_present(replacement_family)
                .unwrap();
        }
        CreateRetryAmbiguity::ReplacementUnlisted => {
            let mut names = Midge::load_databases(&tx).unwrap();
            names.retain(|name| name != DATABASE);
            assert_eq!(names, [midge.default_database.as_str()]);
            Midge::save_databases(&mut tx, &names).unwrap();
        }
        CreateRetryAmbiguity::WrongJournalKey => {
            Midge::delete_lifecycle_record(&mut tx, &intent.operation_id).unwrap();
            tx.put(
                key_encoding::database_lifecycle_key(&Uuid::new_v4().to_string()),
                serde_json::to_vec(&intent).unwrap(),
                None,
            )
            .unwrap();
        }
        CreateRetryAmbiguity::InvalidStagedFamily => {
            "db-invalid".clone_into(&mut intent.physical_family);
            Midge::write_lifecycle_record(&mut tx, &intent).unwrap();
        }
        CreateRetryAmbiguity::InvalidReplacementFamily => {
            let meta = DatabaseMeta::with_physical_family(DATABASE, None, "cf0");
            Midge::write_database_metadata(&mut tx, &meta).unwrap();
        }
        CreateRetryAmbiguity::RegistryKeyMismatch => {
            let meta = DatabaseMeta::with_physical_family("other", None, replacement_family);
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

/// Replaces one intent with two overlapping family owners in selectable journal order.
#[doc(hidden)]
pub fn persist_overlapping_intents(
    midge: &Midge,
    seed: &CreateRetrySeed,
    replacement_family: &str,
    sibling_first: bool,
) {
    let first = "00000000-0000-4000-8000-000000000001";
    let last = "ffffffff-ffff-4fff-bfff-fffffffffffe";
    let mut tx = midge.begin_schema_rw_tx().unwrap();
    Midge::delete_lifecycle_record(&mut tx, &seed.operation_id).unwrap();
    let operation_id = if sibling_first { last } else { first };
    let intent = DatabaseLifecycleRecord {
        operation_id: operation_id.to_owned(),
        kind: DatabaseLifecycleKind::Create,
        name: DATABASE.to_owned(),
        physical_family: seed.staged_family.clone(),
    };
    let sibling = DatabaseLifecycleRecord {
        operation_id: if sibling_first { first } else { last }.to_owned(),
        kind: if sibling_first {
            DatabaseLifecycleKind::Create
        } else {
            DatabaseLifecycleKind::Drop
        },
        name: "unpublished".to_owned(),
        physical_family: replacement_family.to_owned(),
    };
    Midge::write_lifecycle_record(&mut tx, &intent).unwrap();
    Midge::write_lifecycle_record(&mut tx, &sibling).unwrap();
    tx.commit(midge.write_options_sync()).unwrap();
}

/// Persists an orphan create and replacement drop beside the retry intent.
#[doc(hidden)]
pub fn persist_independent_intents(midge: &Midge, replacement_family: &str) -> String {
    let orphan = DatabaseMeta::new("abandoned", None);
    let replacement = DatabaseMeta::with_physical_family(DATABASE, None, replacement_family);
    let mut tx = midge.begin_schema_rw_tx().unwrap();
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
    tx.commit(midge.write_options_sync()).unwrap();
    orphan.physical_family
}

/// Removes the test orphan family after its mixed-journal recovery.
#[doc(hidden)]
pub fn create_orphan_family(midge: &Midge, family: &str) {
    midge.create_physical_family(family).unwrap();
}

trait LifecycleKindExt {
    fn kind_matches_create(&self) -> bool;
}

impl LifecycleKindExt for DatabaseLifecycleRecord {
    fn kind_matches_create(&self) -> bool {
        matches!(&self.kind, DatabaseLifecycleKind::Create)
    }
}
