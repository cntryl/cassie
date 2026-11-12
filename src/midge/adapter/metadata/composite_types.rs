use super::super::key_encoding;
use super::{CassieError, Midge};
use crate::catalog::{CompositeTypeMeta, COMPOSITE_TYPE_FORMAT_VERSION};

fn validate_composite_type(metadata: &CompositeTypeMeta) -> Result<(), CassieError> {
    if metadata.format_version != COMPOSITE_TYPE_FORMAT_VERSION {
        return Err(CassieError::Unsupported(format!(
            "composite type metadata format {} is unsupported",
            metadata.format_version
        )));
    }
    let ids = [
        metadata.object_id,
        metadata.array_object_id,
        metadata.relation_object_id,
    ];
    if ids.contains(&0)
        || ids.iter().any(|id| u32::try_from(*id).is_err())
        || ids
            .iter()
            .enumerate()
            .any(|(index, id)| ids[..index].contains(id))
    {
        return Err(CassieError::Unsupported(format!(
            "composite type {} has invalid or duplicate object identities",
            metadata.name
        )));
    }
    Ok(())
}

impl Midge {
    /// Persist one composite type in its database family.
    ///
    /// # Errors
    ///
    /// Returns an error when validation, serialization or durable publication fails.
    pub fn put_composite_type(&self, metadata: &CompositeTypeMeta) -> Result<(), CassieError> {
        validate_composite_type(metadata)?;
        let mut tx = self.begin_database_adapter_tx(
            &metadata.database,
            cntryl_midge::TransactionMode::ReadWrite,
        )?;
        tx.set_conflict_policy(cntryl_midge::ConflictPolicy::AbortOnWriteConflict);
        let value = serde_json::to_vec(metadata).map_err(|error| {
            CassieError::Parse(format!("serialize composite type metadata: {error}"))
        })?;
        tx.put(
            key_encoding::composite_type_key(&metadata.name),
            value,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// Load every composite type; malformed or unknown records fail closed.
    ///
    /// # Errors
    ///
    /// Returns an error when a record cannot be decoded or its format is unsupported.
    pub fn list_composite_types(&self) -> Result<Vec<CompositeTypeMeta>, CassieError> {
        let entries = self.scan_database_families(&key_encoding::composite_type_prefix())?;
        let mut types = Vec::with_capacity(entries.len());
        for (_key, raw) in entries {
            let metadata: CompositeTypeMeta = serde_json::from_slice(&raw).map_err(|error| {
                CassieError::Parse(format!("invalid composite type metadata: {error}"))
            })?;
            validate_composite_type(&metadata)?;
            types.push(metadata);
        }
        types.sort_by_key(|metadata| metadata.name.to_ascii_lowercase());
        Ok(types)
    }

    /// Delete one composite type from its database family.
    ///
    /// # Errors
    ///
    /// Returns an error when the transaction cannot be published.
    pub fn delete_composite_type(&self, database: &str, name: &str) -> Result<(), CassieError> {
        let mut tx =
            self.begin_database_adapter_tx(database, cntryl_midge::TransactionMode::ReadWrite)?;
        tx.set_conflict_policy(cntryl_midge::ConflictPolicy::AbortOnWriteConflict);
        tx.delete(key_encoding::composite_type_key(name))
            .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(())
    }
}
