use super::super::key_encoding;
use super::{CassieError, Midge};
use crate::catalog::{DomainTypeMeta, DOMAIN_TYPE_FORMAT_VERSION};

fn validate_domain_type(metadata: &DomainTypeMeta) -> Result<(), CassieError> {
    if metadata.format_version != DOMAIN_TYPE_FORMAT_VERSION {
        return Err(CassieError::Unsupported(format!(
            "domain type metadata format {} is unsupported",
            metadata.format_version
        )));
    }
    let ids = [metadata.object_id, metadata.array_object_id];
    if ids.contains(&0)
        || ids
            .iter()
            .any(|id| crate::catalog::object_identity::normal_oid(*id).is_none())
        || ids[0] == ids[1]
    {
        return Err(CassieError::Unsupported(format!(
            "domain type {} has invalid or duplicate object identities",
            metadata.name
        )));
    }
    Ok(())
}

impl Midge {
    /// Persist one domain type in its database family.
    ///
    /// # Errors
    ///
    /// Returns an error when validation, serialization or durable publication fails.
    pub fn put_domain_type(&self, metadata: &DomainTypeMeta) -> Result<(), CassieError> {
        validate_domain_type(metadata)?;
        let mut tx = self.begin_database_adapter_tx(
            &metadata.database,
            cntryl_midge::TransactionMode::ReadWrite,
        )?;
        tx.set_conflict_policy(cntryl_midge::ConflictPolicy::AbortOnWriteConflict);
        let value = serde_json::to_vec(metadata).map_err(|error| {
            CassieError::Parse(format!("serialize domain type metadata: {error}"))
        })?;
        tx.put(key_encoding::domain_type_key(&metadata.name), value, None)
            .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// Load every domain type; malformed or unknown records fail closed.
    ///
    /// # Errors
    ///
    /// Returns an error when a record cannot be decoded or its format is unsupported.
    pub fn list_domain_types(&self) -> Result<Vec<DomainTypeMeta>, CassieError> {
        let entries = self.scan_database_families(&key_encoding::domain_type_prefix())?;
        let mut types = Vec::with_capacity(entries.len());
        for (_key, raw) in entries {
            let metadata: DomainTypeMeta = serde_json::from_slice(&raw).map_err(|error| {
                CassieError::Parse(format!("invalid domain type metadata: {error}"))
            })?;
            validate_domain_type(&metadata)?;
            types.push(metadata);
        }
        types.sort_by_key(|metadata| metadata.name.to_ascii_lowercase());
        Ok(types)
    }

    /// Delete one domain type from its database family.
    ///
    /// # Errors
    ///
    /// Returns an error when the transaction cannot be published.
    pub fn delete_domain_type(&self, database: &str, name: &str) -> Result<(), CassieError> {
        let mut tx =
            self.begin_database_adapter_tx(database, cntryl_midge::TransactionMode::ReadWrite)?;
        tx.set_conflict_policy(cntryl_midge::ConflictPolicy::AbortOnWriteConflict);
        tx.delete(key_encoding::domain_type_key(name))
            .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(())
    }
}
