use super::super::key_encoding;
use super::{CassieError, Midge};
use crate::catalog::ExtensionMeta;

fn validate_extension_profile(metadata: &ExtensionMeta) -> Result<(), CassieError> {
    let selected_version = match metadata.name.to_ascii_lowercase().as_str() {
        "pgcrypto" => "1.4",
        "vector" => "0.8.7",
        _ => {
            return Err(CassieError::Unsupported(format!(
                "extension {} is outside the selected built-in profiles",
                metadata.name
            )));
        }
    };
    if metadata.version != selected_version {
        return Err(CassieError::Unsupported(format!(
            "extension {} version {} is outside the selected built-in profile {selected_version}",
            metadata.name, metadata.version
        )));
    }
    Ok(())
}

impl Midge {
    /// Persist one selected extension profile in the active database family.
    ///
    /// # Errors
    ///
    /// Returns an error when serialization or durable publication fails.
    pub fn put_extension(&self, metadata: &ExtensionMeta) -> Result<(), CassieError> {
        validate_extension_profile(metadata)?;
        if metadata.format_version != 1 {
            return Err(CassieError::Unsupported(format!(
                "extension metadata format {} is unsupported",
                metadata.format_version
            )));
        }
        let mut tx = self.begin_database_adapter_tx(
            &metadata.database,
            cntryl_midge::TransactionMode::ReadWrite,
        )?;
        tx.set_conflict_policy(cntryl_midge::ConflictPolicy::AbortOnWriteConflict);
        let value = serde_json::to_vec(metadata).map_err(|error| {
            CassieError::Parse(format!("serialize extension metadata: {error}"))
        })?;
        tx.put(key_encoding::extension_key(&metadata.name), value, None)
            .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// Load every installed extension profile; malformed or unknown records fail closed.
    ///
    /// # Errors
    ///
    /// Returns an error when a record cannot be decoded or its format is unsupported.
    pub fn list_extensions(&self) -> Result<Vec<ExtensionMeta>, CassieError> {
        let entries = self.scan_database_families(&key_encoding::extension_prefix())?;
        let mut extensions = Vec::with_capacity(entries.len());
        for (_key, raw) in entries {
            let metadata: ExtensionMeta = serde_json::from_slice(&raw).map_err(|error| {
                CassieError::Parse(format!("invalid extension metadata: {error}"))
            })?;
            if metadata.format_version != 1 {
                return Err(CassieError::Unsupported(format!(
                    "extension metadata format {} is unsupported",
                    metadata.format_version
                )));
            }
            validate_extension_profile(&metadata)?;
            extensions.push(metadata);
        }
        extensions.sort_by_key(|metadata| {
            (
                metadata.database.to_ascii_lowercase(),
                metadata.name.to_ascii_lowercase(),
            )
        });
        Ok(extensions)
    }

    /// Delete one extension and its selected dependent views in one database-family transaction.
    ///
    /// # Errors
    ///
    /// Returns an error when the transaction cannot be published.
    pub fn delete_extension_and_views(
        &self,
        database: &str,
        name: &str,
        view_names: &[String],
    ) -> Result<(), CassieError> {
        let mut tx =
            self.begin_database_adapter_tx(database, cntryl_midge::TransactionMode::ReadWrite)?;
        tx.set_conflict_policy(cntryl_midge::ConflictPolicy::AbortOnWriteConflict);
        tx.delete(key_encoding::extension_key(name))
            .map_err(CassieError::from)?;
        for view in view_names {
            tx.delete(key_encoding::view_key(view))
                .map_err(CassieError::from)?;
        }
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(())
    }
}
