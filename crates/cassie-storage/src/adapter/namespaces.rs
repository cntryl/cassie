//! Namespace creation and stored metadata lookup.
use super::{collect_scan, CassieError, Midge, NamespaceMeta, Query};
use crate::catalog::{
    canonical_relation_name, canonical_schema_name, is_system_schema, local_name, parse_name,
    relation_database_name, ParsedName,
};

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

impl Midge {
    /// The canonical stored name for `collection` in the default database and schema.
    #[must_use]
    pub fn canonical_collection_name(&self, collection: &str) -> String {
        match parse_name(collection) {
            Ok(ParsedName::Unqualified(name)) => canonical_relation_name(
                &self.default_database,
                crate::catalog::DEFAULT_SCHEMA,
                &name,
            ),
            Ok(ParsedName::SchemaQualified { schema, name }) => {
                if crate::catalog::is_system_schema(&schema) {
                    format!("{schema}.{name}")
                } else {
                    canonical_relation_name(&self.default_database, &schema, &name)
                }
            }
            Ok(ParsedName::DatabaseQualified {
                database,
                schema,
                name,
            }) => canonical_relation_name(&database, &schema, &name),
            Err(_) => collection.to_string(),
        }
    }

    pub(crate) fn canonical_namespace_name(&self, namespace: &str) -> String {
        match parse_name(namespace) {
            Ok(ParsedName::Unqualified(name)) if is_system_schema(&name) => name,
            Ok(ParsedName::Unqualified(name)) => {
                canonical_schema_name(&self.default_database, &name)
            }
            Ok(ParsedName::SchemaQualified { schema, name }) => {
                canonical_schema_name(&schema, &name)
            }
            Ok(ParsedName::DatabaseQualified {
                database, schema, ..
            }) => canonical_schema_name(&database, &schema),
            Err(_) => namespace.to_string(),
        }
    }

    pub(crate) fn display_collection_name(&self, collection: &str) -> String {
        let canonical = self.canonical_collection_name(collection);
        if relation_database_name(&canonical)
            .is_some_and(|database| database.eq_ignore_ascii_case(&self.default_database))
        {
            local_name(&canonical)
        } else {
            canonical
        }
    }
}
