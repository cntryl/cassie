use std::collections::HashSet;

use super::{
    collect_scan, key_encoding, CassieError, ColumnFamilyHandle, DatabaseLifecycleKind,
    DatabaseLifecycleRecord, DatabaseMeta, Midge, Query, TransactionMode, Uuid,
};

enum FamilyRecovery {
    Keep,
    EnsurePresent,
    DropIfPresent,
}

impl Midge {
    pub(super) fn replay_database_lifecycle_under_guard(
        &self,
        schema: &ColumnFamilyHandle,
    ) -> Result<(), CassieError> {
        let tx = self.begin_schema_tx_for_handle(schema, TransactionMode::ReadOnly)?;
        let entries = collect_scan(
            tx.scan(&Query::new().prefix(key_encoding::database_lifecycle_prefix().into()))
                .map_err(CassieError::from)?,
        )?;
        let mut records = Vec::new();
        for (key, value) in entries {
            let record: DatabaseLifecycleRecord =
                serde_json::from_slice(&value).map_err(|error| {
                    CassieError::StorageBootstrap(format!(
                        "invalid database lifecycle journal record: {error}"
                    ))
                })?;
            record.validate_key_and_create_identity(&key)?;
            records.push(record);
        }
        drop(tx);

        // Decide against the recovered state before an earlier journal entry can
        // create or drop a family which a later mismatch requires us to inspect.
        let actions = records
            .iter()
            .map(|record| self.family_recovery(schema, record, &records))
            .collect::<Result<Vec<_>, _>>()?;
        for (record, action) in records.into_iter().zip(actions) {
            match action {
                FamilyRecovery::Keep => {}
                FamilyRecovery::EnsurePresent => {
                    self.create_physical_family(&record.physical_family)?;
                }
                FamilyRecovery::DropIfPresent => {
                    self.drop_physical_family_if_present(&record.physical_family)?;
                }
            }
            let mut cleanup =
                self.begin_schema_tx_for_handle(schema, TransactionMode::ReadWrite)?;
            Self::delete_lifecycle_record(&mut cleanup, &record.operation_id)?;
            cleanup
                .commit(self.write_options_sync())
                .map_err(CassieError::from)?;
        }
        Ok(())
    }

    fn family_recovery(
        &self,
        schema: &ColumnFamilyHandle,
        record: &DatabaseLifecycleRecord,
        journal: &[DatabaseLifecycleRecord],
    ) -> Result<FamilyRecovery, CassieError> {
        let metadata = self.load_database_without_layout(&record.name, schema)?;
        match (&record.kind, metadata) {
            (_, None) => Ok(FamilyRecovery::DropIfPresent),
            (DatabaseLifecycleKind::Drop, Some(_)) => Ok(FamilyRecovery::Keep),
            (DatabaseLifecycleKind::Create, Some(owner))
                if owner.physical_family == record.physical_family =>
            {
                Ok(FamilyRecovery::EnsurePresent)
            }
            (DatabaseLifecycleKind::Create, Some(replacement)) => {
                self.validate_replacement_owner(schema, record, &replacement)?;
                if journal.iter().any(|other| {
                    other.name != record.name
                        && (other.physical_family == record.physical_family
                            || other.physical_family == replacement.physical_family)
                }) {
                    return Err(record
                        .family_mismatch("another database journal references the same family"));
                }
                Ok(FamilyRecovery::Keep)
            }
        }
    }

    // Called only for a mismatched Create during cold bootstrap. The schema
    // publication guard covers these observations through the journal commit.
    fn validate_replacement_owner(
        &self,
        schema: &ColumnFamilyHandle,
        record: &DatabaseLifecycleRecord,
        replacement: &DatabaseMeta,
    ) -> Result<(), CassieError> {
        if replacement.name != record.name || !is_generated_family(&replacement.physical_family) {
            return Err(record.family_mismatch("invalid replacement database identity"));
        }
        let tx = self.begin_schema_tx_for_handle(schema, TransactionMode::ReadOnly)?;
        let listed = Self::load_databases(&tx)?;
        let entries = collect_scan(
            tx.scan(&Query::new().prefix(Self::database_prefix().into()))
                .map_err(CassieError::from)?,
        )?;
        let mut names = HashSet::new();
        for (key, value) in entries {
            let owner: DatabaseMeta = serde_json::from_slice(&value).map_err(|error| {
                CassieError::StorageBootstrap(format!("invalid database registry record: {error}"))
            })?;
            if key != Self::database_key(&owner.name) {
                return Err(record.family_mismatch("database registry key disagrees with metadata"));
            }
            let exact_owner = owner.name == record.name;
            if (owner.physical_family == replacement.physical_family) != exact_owner
                || (!exact_owner && owner.name.eq_ignore_ascii_case(&record.name))
            {
                return Err(record.family_mismatch("conflicting replacement ownership"));
            }
            if owner.physical_family == record.physical_family {
                return Err(record.family_mismatch("journaled family has a registered owner"));
            }
            names.insert(owner.name);
        }

        // With an empty list, bootstrap discovers owners directly from the
        // keyed records. A nonempty registry must resolve its listed owners.
        if !names.contains(&record.name)
            || (!listed.is_empty()
                && (listed.iter().filter(|name| *name == &record.name).count() != 1
                    || listed.iter().any(|name| !names.contains(name))))
        {
            return Err(record.family_mismatch("replacement registry membership is ambiguous"));
        }
        let active = self
            .engine
            .list_column_families()
            .map_err(CassieError::from)?;
        if active
            .iter()
            .any(|family| family.name() == record.physical_family)
            || !active
                .iter()
                .any(|family| family.name() == replacement.physical_family)
        {
            return Err(record.family_mismatch("active families do not confirm the replacement"));
        }
        Ok(())
    }
}

impl DatabaseLifecycleRecord {
    fn validate_key_and_create_identity(&self, key: &[u8]) -> Result<(), CassieError> {
        if key != key_encoding::database_lifecycle_key(&self.operation_id) {
            return Err(CassieError::StorageBootstrap(
                "database lifecycle journal key disagrees with operation ID".to_owned(),
            ));
        }
        if matches!(self.kind, DatabaseLifecycleKind::Create)
            && (self.name.is_empty()
                || !is_generated_family(&self.physical_family)
                || generated_uuid(&self.operation_id)
                    .is_none_or(|id| id.to_string() != self.operation_id))
        {
            return Err(CassieError::StorageBootstrap(
                "invalid database create lifecycle identity".to_owned(),
            ));
        }
        Ok(())
    }

    fn family_mismatch(&self, reason: &str) -> CassieError {
        CassieError::StorageBootstrap(format!(
            "database '{}' lifecycle family mismatch: {reason}",
            self.name
        ))
    }
}

fn generated_uuid(raw: &str) -> Option<Uuid> {
    Uuid::parse_str(raw).ok().filter(|id| {
        id.get_version() == Some(uuid::Version::Random)
            && id.get_variant() == uuid::Variant::RFC4122
    })
}

fn is_generated_family(name: &str) -> bool {
    let Some(raw) = name.strip_prefix("db-") else {
        return false;
    };
    generated_uuid(raw).is_some_and(|id| id.simple().to_string() == raw)
}
