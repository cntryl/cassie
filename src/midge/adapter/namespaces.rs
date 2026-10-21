//! Namespace creation and stored metadata lookup.
use super::{collect_scan, CassieError, Midge, NamespaceMeta, Query};

impl Midge {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    /// Returns the stored metadata, including the stable object identity.
    pub fn create_namespace(&self, namespace: &str) -> Result<NamespaceMeta, CassieError> {
        let canonical = self.canonical_namespace_name(namespace);
        if let Some(database) = crate::catalog::schema_database_name(&canonical) {
            let _ = self.database_family(&database)?;
        }
        let mut tx = self.begin_namespace_rw_tx(&canonical)?;
        let namespace_key = Self::namespace_key(&canonical);
        let stored = tx
            .get(&namespace_key)
            .map_err(CassieError::from)?
            .map(|raw| serde_json::from_slice::<NamespaceMeta>(&raw))
            .transpose()
            .map_err(|error| CassieError::Parse(format!("invalid namespace metadata: {error}")))?;
        let metadata = match stored {
            Some(metadata) if metadata.object_id != 0 => metadata,
            stored => {
                let mut metadata = stored.unwrap_or_else(|| NamespaceMeta::new(&canonical, None));
                metadata.name.clone_from(&canonical);
                metadata.object_id = self.allocate_object_id()?;
                let serialized = serde_json::to_vec(&metadata)
                    .map_err(|error| CassieError::Parse(error.to_string()))?;
                tx.put(namespace_key, serialized, None)
                    .map_err(CassieError::from)?;
                metadata
            }
        };

        let mut namespaces = Self::load_namespaces(&tx)?;
        if !namespaces
            .iter()
            .any(|entry| self.canonical_namespace_name(entry) == canonical)
        {
            namespaces.push(canonical.clone());
            namespaces.sort();
            Self::save_namespaces(&mut tx, &namespaces)?;
        }

        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(metadata)
    }

    /// Stored metadata for one namespace, if it exists.
    ///
    /// # Errors
    ///
    /// Returns an error when the stored metadata cannot be read.
    pub fn namespace_metadata(
        &self,
        namespace: &str,
    ) -> Result<Option<NamespaceMeta>, CassieError> {
        let canonical = self.canonical_namespace_name(namespace);
        let tx = self.database_tx_for_namespace(&canonical, super::TransactionMode::ReadOnly)?;
        let parse = |raw: &[u8]| {
            serde_json::from_slice::<NamespaceMeta>(raw)
                .map_err(|error| CassieError::Parse(format!("invalid namespace metadata: {error}")))
        };
        if let Some(raw) = tx
            .get(&Self::namespace_key(&canonical))
            .map_err(CassieError::from)?
        {
            return parse(&raw).map(Some);
        }
        let entries = collect_scan(
            tx.scan(&Query::new().prefix(Self::namespace_prefix().into()))
                .map_err(CassieError::from)?,
        )?;
        for (_key, raw) in entries {
            let metadata = parse(&raw)?;
            if self.canonical_namespace_name(&metadata.name) == canonical {
                return Ok(Some(metadata));
            }
        }
        Ok(None)
    }
}
