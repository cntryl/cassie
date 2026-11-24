use super::super::key_encoding;
use super::{CassieError, Midge};
use crate::catalog::enum_reconcile::EnumPropagation;
use crate::catalog::{EnumTypeMeta, ENUM_TYPE_FORMAT_VERSION};

/// Rejects a descriptor that could not be reloaded: an unknown format,
/// missing or duplicate object identities, or members that are not a strictly
/// increasing run of distinct labels and identities.
fn validate_enum_type(metadata: &EnumTypeMeta) -> Result<(), CassieError> {
    if metadata.format_version != ENUM_TYPE_FORMAT_VERSION {
        return Err(CassieError::Unsupported(format!(
            "enum type metadata format {} is unsupported",
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
            "enum type {} has invalid or duplicate object identities",
            metadata.name
        )));
    }
    let members = &metadata.members;
    let ordered = members
        .windows(2)
        .all(|pair| pair[0].sort_order < pair[1].sort_order);
    let distinct = members.iter().enumerate().all(|(index, member)| {
        member.member_id != 0
            && !members[..index].iter().any(|earlier| {
                earlier.member_id == member.member_id || earlier.label == member.label
            })
    });
    if !(ordered && distinct) {
        return Err(CassieError::Unsupported(format!(
            "enum type {} has unordered or duplicate members",
            metadata.name
        )));
    }
    Ok(())
}

impl Midge {
    /// Persist one enum type in its database family.
    ///
    /// # Errors
    ///
    /// Returns an error when validation, serialization or durable publication fails.
    pub fn put_enum_type(&self, metadata: &EnumTypeMeta) -> Result<(), CassieError> {
        validate_enum_type(metadata)?;
        let mut tx = self.begin_database_adapter_tx(
            &metadata.database,
            cntryl_midge::TransactionMode::ReadWrite,
        )?;
        tx.set_conflict_policy(cntryl_midge::ConflictPolicy::AbortOnWriteConflict);
        let value = serde_json::to_vec(metadata).map_err(|error| {
            CassieError::Parse(format!("serialize enum type metadata: {error}"))
        })?;
        tx.put(key_encoding::enum_type_key(&metadata.name), value, None)
            .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// Publish one altered enum type atomically: the descriptor (deleting the
    /// record stored under `replaced_name` after a rename or schema move),
    /// every table column that embeds it, and every view output that does.
    ///
    /// # Errors
    ///
    /// Returns an error when validation fails or the transaction cannot be
    /// published; nothing from the group is then visible.
    pub fn publish_enum_group(
        &self,
        metadata: &EnumTypeMeta,
        replaced_name: Option<&str>,
        propagation: &EnumPropagation,
    ) -> Result<(), CassieError> {
        validate_enum_type(metadata)?;
        let mut tables: Vec<String> = propagation
            .columns
            .iter()
            .map(|column| self.canonical_collection_name(&column.table))
            .collect();
        tables.sort();
        tables.dedup();
        let gates: Vec<_> = tables
            .iter()
            .map(|table| self.collection_write_gate(table))
            .collect();
        let _guards: Vec<_> = gates.iter().map(|gate| gate.lock()).collect();
        let mut tx = self.begin_database_adapter_tx(
            &metadata.database,
            cntryl_midge::TransactionMode::ReadWrite,
        )?;
        tx.set_conflict_policy(cntryl_midge::ConflictPolicy::AbortOnWriteConflict);
        for column in &propagation.columns {
            let table = self.canonical_collection_name(&column.table);
            Self::stage_column_type(&mut tx, &table, &column.column, column.data_type.clone())?;
        }
        for view in &propagation.views {
            crate::sql::definition_guard::view(view)?;
            self.require_definition_version(view.definition_semantics_version)?;
            let mut view = view.clone();
            view.name = self.canonical_collection_name(&view.name);
            let value =
                serde_json::to_vec(&view).map_err(|error| CassieError::Parse(error.to_string()))?;
            tx.put(Self::view_key(&view.name), value, None)
                .map_err(CassieError::from)?;
        }
        let value = serde_json::to_vec(metadata).map_err(|error| {
            CassieError::Parse(format!("serialize enum type metadata: {error}"))
        })?;
        if let Some(old) = replaced_name {
            tx.delete(key_encoding::enum_type_key(old))
                .map_err(CassieError::from)?;
        }
        tx.put(key_encoding::enum_type_key(&metadata.name), value, None)
            .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)
    }

    /// Load every enum type; malformed or unknown records fail closed.
    ///
    /// # Errors
    ///
    /// Returns an error when a record cannot be decoded or its format is unsupported.
    pub fn list_enum_types(&self) -> Result<Vec<EnumTypeMeta>, CassieError> {
        let entries = self.scan_database_families(&key_encoding::enum_type_prefix())?;
        let mut types = Vec::with_capacity(entries.len());
        for (_key, raw) in entries {
            let metadata: EnumTypeMeta = serde_json::from_slice(&raw).map_err(|error| {
                CassieError::Parse(format!("invalid enum type metadata: {error}"))
            })?;
            validate_enum_type(&metadata)?;
            types.push(metadata);
        }
        types.sort_by_key(|metadata| metadata.name.to_ascii_lowercase());
        Ok(types)
    }

    /// Delete one enum type from its database family.
    ///
    /// # Errors
    ///
    /// Returns an error when the transaction cannot be published.
    pub fn delete_enum_type(&self, database: &str, name: &str) -> Result<(), CassieError> {
        let mut tx =
            self.begin_database_adapter_tx(database, cntryl_midge::TransactionMode::ReadWrite)?;
        tx.set_conflict_policy(cntryl_midge::ConflictPolicy::AbortOnWriteConflict);
        tx.delete(key_encoding::enum_type_key(name))
            .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "enum_types_tests.rs"]
mod tests;
