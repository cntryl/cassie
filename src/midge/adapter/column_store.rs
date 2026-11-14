use super::{
    collect_scan, key_encoding, CassieError, CollectionCardinalityStats, CollectionMeta,
    CollectionStorageMode, FieldConstraint, HashSet, IndexKind, IndexMeta, Midge, ProjectionMeta,
    Query, RowFilter, RowSchema, Schema,
};
#[cfg(test)]
mod golden_tests;
mod rows;
pub(super) mod validation;

type InitialCollectionPublication = (bool, u64, Vec<crate::catalog::ObjectCommentMeta>);
type ClonePublication = (u64, Vec<crate::catalog::ObjectCommentMeta>);
type IndexesAndComments<'a> = (&'a [IndexMeta], &'a [(String, String)]);

#[derive(Clone, Copy, Default)]
struct InitialCollectionContents<'a> {
    constraints: Option<&'a [FieldConstraint]>,
    sequences: Option<&'a [crate::catalog::SequenceMeta]>,
    indexes: Option<&'a [IndexMeta]>,
    comments: Option<&'a [(String, String)]>,
}

#[derive(Clone, Copy)]
pub(crate) struct ColumnStoreScanRequest<'a> {
    pub traversal: super::RowTraversal,
    pub collection: &'a str,
    pub row_schema: &'a RowSchema,
    pub batch_size: usize,
    pub projection: Option<&'a HashSet<String>>,
    pub filter: Option<&'a RowFilter>,
    pub limit: usize,
}

impl Midge {
    #[doc(hidden)]
    pub fn column_store_prefix_for_diagnostics(
        &self,
        collection: &str,
    ) -> Result<Vec<u8>, CassieError> {
        let relation_id = self
            .collection_metadata(collection)?
            .ok_or_else(|| CassieError::CollectionNotFound(collection.to_string()))?
            .storage_id;
        Ok(Self::column_store_collection_prefix(relation_id))
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    /// Returns the relation's stable storage identity, which also derives its
    /// PostgreSQL object identifier.
    pub fn create_collection_with_meta(
        &self,
        name: &str,
        schema: &Schema,
        metadata: &CollectionMeta,
    ) -> Result<u64, CassieError> {
        self.create_collection_with_meta_inner(
            name,
            schema,
            metadata,
            false,
            InitialCollectionContents::default(),
        )
        .map(|(_, storage_id, _)| storage_id)
    }

    /// Creates a collection only when no case-insensitive storage relation with
    /// the same name exists.
    ///
    /// Returns the new storage identity when this call created the collection
    /// and `None` when an existing relation owns the name.
    ///
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub(crate) fn create_collection_with_meta_if_absent(
        &self,
        name: &str,
        schema: &Schema,
        metadata: &CollectionMeta,
    ) -> Result<Option<u64>, CassieError> {
        self.create_collection_with_meta_inner(
            name,
            schema,
            metadata,
            true,
            InitialCollectionContents::default(),
        )
        .map(|(created, storage_id, _)| created.then_some(storage_id))
    }

    /// Creates a collection, initial constraints, and owned identity sequences in one database-family transaction.
    ///
    /// Initial constraints may include nullability, defaults, checks, and identity metadata paired
    /// with its new sequence. Unique and foreign-key constraints remain outside this path because
    /// they require reservation or dependency work.
    ///
    /// # Errors
    ///
    /// Returns an error when validation or storage fails.
    pub(crate) fn create_collection_with_meta_and_constraints_and_sequences_if_absent(
        &self,
        name: &str,
        schema: &Schema,
        metadata: &CollectionMeta,
        constraints: &[FieldConstraint],
        sequences: &[crate::catalog::SequenceMeta],
    ) -> Result<Option<u64>, CassieError> {
        self.validate_initial_column_constraints(name, constraints, sequences)?;
        let initial = InitialCollectionContents {
            constraints: Some(constraints),
            sequences: Some(sequences),
            ..InitialCollectionContents::default()
        };
        let (created, storage_id, _) =
            self.create_collection_with_meta_inner(name, schema, metadata, true, initial)?;
        Ok(created.then_some(storage_id))
    }

    /// Atomically creates an empty table clone with its supported constraints,
    /// sequences, and scalar index metadata in the owning database family.
    ///
    /// The target has no rows, so its scalar indexes have no physical entries
    /// to backfill before publication.
    ///
    /// # Errors
    ///
    /// Returns an error when the initial metadata is invalid or storage fails.
    pub(crate) fn create_collection_with_meta_constraints_sequences_and_indexes_if_absent(
        &self,
        name: &str,
        schema: &Schema,
        metadata: &CollectionMeta,
        constraints: &[FieldConstraint],
        sequences: &[crate::catalog::SequenceMeta],
        indexes_and_comments: IndexesAndComments<'_>,
    ) -> Result<Option<ClonePublication>, CassieError> {
        let (indexes, comments) = indexes_and_comments;
        if comments.is_empty()
            && indexes.is_empty()
            && !constraints
                .iter()
                .any(|constraint| constraint.primary_key || constraint.unique)
        {
            return self
                .create_collection_with_meta_and_constraints_and_sequences_if_absent(
                    name,
                    schema,
                    metadata,
                    constraints,
                    sequences,
                )
                .map(|created| created.map(|id| (id, Vec::new())));
        }
        self.validate_initial_clone_constraints(name, constraints, sequences, indexes)?;
        let initial = InitialCollectionContents {
            constraints: Some(constraints),
            sequences: Some(sequences),
            indexes: Some(indexes),
            comments: Some(comments),
        };
        let (created, storage_id, copied_comments) =
            self.create_collection_with_meta_inner(name, schema, metadata, true, initial)?;
        Ok(created.then_some((storage_id, copied_comments)))
    }

    /// Writes the row schema, projection and cardinality records that are not stored yet.
    fn save_missing_collection_state(
        tx: &mut crate::midge::adapter::AdapterTransaction,
        name: &str,
        row_schema: &RowSchema,
    ) -> Result<(), CassieError> {
        if tx
            .get(&Self::row_schema_key(name))
            .map_err(CassieError::from)?
            .is_none()
        {
            Self::save_row_schema_to_tx(tx, name, row_schema)?;
        }
        if tx
            .get(&Self::projection_key(name))
            .map_err(CassieError::from)?
            .is_none()
        {
            Self::save_projection_metadata_to_tx(
                tx,
                &ProjectionMeta::new(name, row_schema.schema_version),
            )?;
        }
        if tx
            .get(Self::cardinality_key(name).as_slice())
            .map_err(CassieError::from)?
            .is_none()
        {
            Self::save_cardinality_stats_to_tx(tx, name, &CollectionCardinalityStats::default())?;
        }
        Ok(())
    }

    fn create_collection_with_meta_inner(
        &self,
        name: &str,
        schema: &Schema,
        metadata: &CollectionMeta,
        require_absent: bool,
        initial: InitialCollectionContents<'_>,
    ) -> Result<InitialCollectionPublication, CassieError> {
        self.admit_initial_definitions(initial)?;
        self.require_collection_storage_layout(metadata)?;
        let name = self.canonical_collection_name(name);
        let write_gate = self.collection_write_gate(&name);
        let _write_guard = write_gate.lock();
        let mut metadata = metadata.clone();
        metadata.name.clone_from(&name);
        if let Some(database) = crate::catalog::relation_database_name(&name) {
            let _ = self.database_family(&database)?;
        }
        let Some(needs_storage_id) = self.collection_storage_id_needed(&name, require_absent)?
        else {
            return Ok((false, 0, Vec::new()));
        };
        let storage_id = needs_storage_id
            .then(|| self.allocate_object_id())
            .transpose()?;
        let row_type_ids = needs_storage_id
            .then(|| Ok::<_, CassieError>((self.allocate_object_id()?, self.allocate_object_id()?)))
            .transpose()?;
        let mut tx = self.begin_data_rw_tx_for(&name)?;

        let schema_key = Self::collection_schema_key(&name);
        let schema_exists = tx.get(&schema_key).map_err(CassieError::from)?.is_some();
        let existing_metadata = Self::load_collection_metadata_from_tx(&tx, &name)?;
        let storage_name_exists = Self::storage_relation_exists_case_insensitively(&tx, &name)?;
        if require_absent && (schema_exists || existing_metadata.is_some() || storage_name_exists) {
            return Ok((false, 0, Vec::new()));
        }

        self.validate_initial_sequences_absent(&mut tx, initial.sequences.unwrap_or(&[]))?;

        if !schema_exists {
            let schema_bytes = serde_json::to_vec(schema)
                .map_err(|error| CassieError::Parse(error.to_string()))?;
            tx.put(schema_key, schema_bytes, None)
                .map_err(CassieError::from)?;
        }
        let storage_id = if let Some(existing) = existing_metadata.as_ref() {
            existing.storage_id
        } else {
            storage_id.ok_or_else(|| {
                CassieError::StorageRetryable(
                    "midge write conflict: collection metadata changed during schema creation"
                        .into(),
                )
            })?
        };
        metadata.storage_id = storage_id;
        assign_row_type_ids(&mut metadata, existing_metadata.as_ref(), row_type_ids);
        let mut row_schema = RowSchema::from_schema(schema);
        row_schema.relation_id = storage_id;
        Self::save_missing_collection_state(&mut tx, &name, &row_schema)?;
        if existing_metadata.is_none() {
            Self::save_collection_metadata_to_tx(&mut tx, &metadata)?;
        }
        if let Some(constraints) = initial.constraints {
            let value = serde_json::to_vec(constraints)
                .map_err(|error| CassieError::Parse(error.to_string()))?;
            tx.put(Self::constraints_key(&name), value, None)
                .map_err(CassieError::from)?;
        }
        if let Some(sequences) = initial.sequences {
            self.save_initial_sequences_to_tx(&mut tx, sequences)?;
        }
        if let Some(indexes) = initial.indexes {
            Self::save_initial_indexes_to_tx(&mut tx, &name, storage_id, indexes)?;
        }

        let copied_comments = Self::save_initial_column_comments_to_tx(
            &mut tx,
            &name,
            &row_schema,
            initial.comments.unwrap_or(&[]),
        )?;

        let mut collections = Self::load_collections(&tx)?;
        if !collections.iter().any(|entry| entry == &name) {
            collections.push(name.clone());
            collections.sort();
            Self::save_collections(&mut tx, &collections)?;
        }

        super::schema_write_control::pause_before_schema_write_commit(
            super::schema_write_control::SchemaWritePausePoint::CollectionCreate,
        );
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok((
            !schema_exists && existing_metadata.is_none(),
            storage_id,
            copied_comments,
        ))
    }

    fn admit_initial_definitions(
        &self,
        initial: InitialCollectionContents<'_>,
    ) -> Result<(), CassieError> {
        if let Some(constraints) = initial.constraints {
            self.require_constraint_definitions(constraints)?;
        }
        for index in initial.indexes.unwrap_or(&[]) {
            crate::sql::definition_guard::index(index)?;
            self.require_definition_version(index.definition_semantics_version)?;
        }
        Ok(())
    }

    fn save_initial_indexes_to_tx(
        tx: &mut crate::midge::adapter::AdapterTransaction,
        name: &str,
        relation_id: u64,
        indexes: &[IndexMeta],
    ) -> Result<(), CassieError> {
        let mut seen_names = std::collections::HashSet::new();
        for index in indexes {
            let index = index.clone_for_relation(name, &index.name);
            if !seen_names.insert(index.name.to_ascii_lowercase()) {
                return Err(CassieError::CatalogObjectAlreadyExists {
                    kind: crate::app::CatalogObjectKind::Index,
                    name: index.name,
                });
            }
            if !matches!(
                index.kind,
                IndexKind::Scalar | IndexKind::FullText | IndexKind::TimeSeries
            ) {
                return Err(CassieError::Unsupported(
                    "initial table clone publication supports scalar, full-text and time-series indexes only"
                        .to_string(),
                ));
            }
            crate::sql::definition_guard::index(&index)?;
            let index_key = Self::index_key(name, &index.name);
            if tx.get(&index_key).map_err(CassieError::from)?.is_some() {
                return Err(CassieError::CatalogObjectAlreadyExists {
                    kind: crate::app::CatalogObjectKind::Index,
                    name: index.name,
                });
            }
            Self::write_initial_index_metadata(tx, &index_key, relation_id, index)?;
        }
        Ok(())
    }

    fn write_initial_index_metadata(
        tx: &mut crate::midge::adapter::AdapterTransaction,
        index_key: &[u8],
        relation_id: u64,
        mut index: IndexMeta,
    ) -> Result<(), CassieError> {
        let index_storage_id = Self::allocate_object_id_to_tx(tx)?;
        let index_name = index.name.clone();
        index.set_storage_ids(relation_id, index_storage_id);
        index.set_catalog_oid_if_missing(crate::catalog::object_identity::stable_index_oid(
            &index_name,
        ));
        if index.unique {
            index.set_unique_reservation_name_if_missing(&index_name);
        }
        tx.put(
            index_key.to_vec(),
            serde_json::to_vec(&index).map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)
    }

    fn collection_storage_id_needed(
        &self,
        name: &str,
        require_absent: bool,
    ) -> Result<Option<bool>, CassieError> {
        let tx = self.database_tx_for_collection(name, super::TransactionMode::ReadOnly)?;
        let schema_exists = tx
            .get(&Self::collection_schema_key(name))
            .map_err(CassieError::from)?
            .is_some();
        let existing_metadata = Self::load_collection_metadata_from_tx(&tx, name)?;
        let storage_name_exists = Self::storage_relation_exists_case_insensitively(&tx, name)?;
        if require_absent && (schema_exists || existing_metadata.is_some() || storage_name_exists) {
            return Ok(None);
        }
        Ok(Some(existing_metadata.is_none()))
    }

    fn validate_initial_column_constraints(
        &self,
        table: &str,
        constraints: &[FieldConstraint],
        sequences: &[crate::catalog::SequenceMeta],
    ) -> Result<(), CassieError> {
        crate::sql::definition_guard::constraints(constraints)?;
        if constraints.iter().any(|constraint| {
            constraint.primary_key
                || constraint.primary_key_name.is_some()
                || constraint.primary_key_ordinal.is_some()
                || constraint.unique
                || constraint.unique_name.is_some()
                || constraint.unique_ordinal.is_some()
                || constraint.foreign_key_name.is_some()
                || constraint.foreign_key_ordinal.is_some()
                || constraint.foreign_key_on_delete.is_some()
                || constraint.foreign_key_on_update.is_some()
                || constraint.references_table.is_some()
                || constraint.references_field.is_some()
                || (constraint.default_sequence_owned.is_owned() != constraint.identity.is_some())
                || (constraint.identity_options.is_some() && constraint.identity.is_none())
        }) {
            return Err(CassieError::Unsupported(
                "initial collection publication supports only NOT NULL, CHECK, default, and identity metadata"
                    .to_string(),
            ));
        }
        let matches_sequence =
            |constraint: &FieldConstraint, sequence: &crate::catalog::SequenceMeta| {
                constraint.identity.is_some()
                    && constraint.default_sequence.as_deref().is_some_and(|name| {
                        self.canonical_collection_name(&sequence.name)
                            == self.canonical_collection_name(name)
                            && sequence.owned_by.as_ref().is_some_and(|owner| {
                                self.canonical_collection_name(&owner.table)
                                    == self.canonical_collection_name(table)
                                    && crate::sql::ColumnIdentifierPath::stored_field_key(
                                        &owner.column,
                                    ) == crate::sql::ColumnIdentifierPath::stored_field_key(
                                        &constraint.field,
                                    )
                            })
                    })
            };
        if constraints
            .iter()
            .filter(|constraint| constraint.identity.is_some())
            .any(|constraint| {
                !sequences
                    .iter()
                    .any(|sequence| matches_sequence(constraint, sequence))
            })
            || sequences.iter().any(|sequence| {
                !constraints
                    .iter()
                    .any(|constraint| matches_sequence(constraint, sequence))
            })
        {
            return Err(CassieError::Unsupported(
                "initial identity metadata requires a matching target-owned sequence".to_string(),
            ));
        }
        Ok(())
    }

    fn validate_initial_clone_constraints(
        &self,
        table: &str,
        constraints: &[FieldConstraint],
        sequences: &[crate::catalog::SequenceMeta],
        indexes: &[IndexMeta],
    ) -> Result<(), CassieError> {
        let mut initial_constraints = constraints.to_vec();
        for constraint in &mut initial_constraints {
            constraint.primary_key = false;
            constraint.primary_key_name = None;
            constraint.primary_key_ordinal = None;
            constraint.unique = false;
            constraint.unique_name = None;
            constraint.unique_ordinal = None;
            constraint.constraint_index_name = None;
            constraint.constraint_index_include_fields.clear();
            constraint.constraint_index_options.clear();
        }
        self.validate_initial_column_constraints(table, &initial_constraints, sequences)?;

        for constraint in constraints
            .iter()
            .filter(|constraint| constraint.primary_key || constraint.unique)
        {
            let key_name = if constraint.primary_key {
                constraint.primary_key_name.as_ref()
            } else {
                constraint.unique_name.as_ref()
            };
            let expected_fields = if let Some(key_name) = key_name {
                let mut members = constraints
                    .iter()
                    .filter(|candidate| {
                        let candidate_name = if constraint.primary_key && candidate.primary_key {
                            candidate.primary_key_name.as_ref()
                        } else if !constraint.primary_key && candidate.unique {
                            candidate.unique_name.as_ref()
                        } else {
                            None
                        };
                        candidate_name.is_some_and(|name| name.eq_ignore_ascii_case(key_name))
                    })
                    .collect::<Vec<_>>();
                members.sort_by_key(|member| {
                    if constraint.primary_key {
                        member.primary_key_ordinal
                    } else {
                        member.unique_ordinal
                    }
                    .unwrap_or(u32::MAX)
                });
                members
                    .into_iter()
                    .map(|member| member.field.as_str())
                    .collect::<Vec<_>>()
            } else {
                vec![constraint.field.as_str()]
            };
            if !indexes.iter().any(|index| {
                index.unique
                    && index.can_back_unique_constraint()
                    && index
                        .normalized_fields()
                        .iter()
                        .map(String::as_str)
                        .eq(expected_fields.iter().copied())
            }) {
                return Err(CassieError::Unsupported(
                    "initial table clone publication requires an index for each copied primary or unique constraint"
                        .to_string(),
                ));
            }
        }
        Ok(())
    }

    fn validate_initial_sequences_absent(
        &self,
        tx: &mut crate::midge::adapter::AdapterTransaction,
        sequences: &[crate::catalog::SequenceMeta],
    ) -> Result<(), CassieError> {
        let mut sequence_names = std::collections::HashSet::new();
        for sequence in sequences {
            let sequence_name = self.canonical_collection_name(&sequence.name);
            if !sequence_names.insert(sequence_name.clone())
                || tx
                    .get(&super::key_encoding::sequence_key(&sequence_name))
                    .map_err(CassieError::from)?
                    .is_some()
            {
                return Err(CassieError::CatalogObjectAlreadyExists {
                    kind: crate::app::CatalogObjectKind::Sequence,
                    name: sequence_name,
                });
            }
        }
        Ok(())
    }

    fn save_initial_sequences_to_tx(
        &self,
        tx: &mut crate::midge::adapter::AdapterTransaction,
        sequences: &[crate::catalog::SequenceMeta],
    ) -> Result<(), CassieError> {
        for sequence in sequences {
            let mut sequence = sequence.clone();
            sequence.name = self.canonical_collection_name(&sequence.name);
            if let Some(owner) = sequence.owned_by.as_mut() {
                owner.table = self.canonical_collection_name(&owner.table);
            }
            let key = super::key_encoding::sequence_key(&sequence.name);
            let value = serde_json::to_vec(&sequence)
                .map_err(|error| CassieError::Parse(error.to_string()))?;
            tx.put(key, value, None).map_err(CassieError::from)?;
        }
        Ok(())
    }

    fn storage_relation_exists_case_insensitively(
        tx: &crate::midge::adapter::AdapterTransaction,
        name: &str,
    ) -> Result<bool, CassieError> {
        let collections = Self::load_collections(tx)?;
        if collections
            .iter()
            .any(|stored| stored.eq_ignore_ascii_case(name))
        {
            return Ok(true);
        }

        let schema_prefix = Self::schema_collection_prefix();
        let schema_entries = collect_scan(
            tx.scan(&Query::new().prefix(schema_prefix.clone().into()))
                .map_err(CassieError::from)?,
        )?;
        Ok(schema_entries.iter().any(|(key, _)| {
            key_encoding::utf8_suffix_after_prefix(key, &schema_prefix)
                .is_some_and(|stored| stored.eq_ignore_ascii_case(name))
        }))
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn collection_metadata(&self, name: &str) -> Result<Option<CollectionMeta>, CassieError> {
        let name = self.canonical_collection_name(name);
        let tx = self.database_tx_for_collection(&name, super::TransactionMode::ReadOnly)?;
        if let Some(metadata) = Self::load_collection_metadata_from_tx(&tx, &name)? {
            return Ok(Some(metadata));
        }
        let exists = tx
            .get(&Self::collection_schema_key(&name))
            .map_err(CassieError::from)?
            .is_some();
        Ok(exists.then(|| CollectionMeta::new(&name, None)))
    }

    pub(crate) fn storage_mode_for_collection(
        &self,
        collection: &str,
    ) -> Result<CollectionStorageMode, CassieError> {
        Ok(self
            .collection_metadata(collection)?
            .map_or(CollectionStorageMode::RowStore, |metadata| {
                metadata.storage_mode
            }))
    }

    pub(crate) fn collection_uses_column_store(
        &self,
        collection: &str,
    ) -> Result<bool, CassieError> {
        Ok(self
            .storage_mode_for_collection(collection)?
            .uses_column_store_storage())
    }

    pub(crate) fn load_collection_metadata_from_tx(
        tx: &crate::midge::adapter::AdapterTransaction,
        name: &str,
    ) -> Result<Option<CollectionMeta>, CassieError> {
        let Some(raw) = tx
            .get(&Self::collection_metadata_key(name))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        let mut metadata: CollectionMeta = serde_json::from_slice(&raw)
            .map_err(|error| CassieError::Parse(format!("invalid collection metadata: {error}")))?;
        metadata.name = name.to_string();
        Ok(Some(metadata))
    }

    pub(crate) fn save_collection_metadata_to_tx(
        tx: &mut crate::midge::adapter::AdapterTransaction,
        metadata: &CollectionMeta,
    ) -> Result<(), CassieError> {
        let value =
            serde_json::to_vec(metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::collection_metadata_key(&metadata.name), value, None)
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub(crate) fn delete_collection_metadata_to_tx(
        tx: &mut crate::midge::adapter::AdapterTransaction,
        name: &str,
    ) -> Result<(), CassieError> {
        tx.delete(Self::collection_metadata_key(name))
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub(crate) fn rename_collection_metadata_to_tx(
        tx: &mut crate::midge::adapter::AdapterTransaction,
        current_name: &str,
        next_name: &str,
    ) -> Result<(), CassieError> {
        let metadata = Self::load_collection_metadata_from_tx(tx, current_name)?
            .unwrap_or_else(|| CollectionMeta::new(current_name, None));
        tx.delete(Self::collection_metadata_key(current_name))
            .map_err(CassieError::from)?;
        let mut renamed = metadata;
        renamed.name = next_name.to_string();
        Self::save_collection_metadata_to_tx(tx, &renamed)
    }

    fn collection_metadata_key(name: &str) -> Vec<u8> {
        key_encoding::collection_metadata_key(name)
    }

    pub(crate) fn column_store_collection_prefix(relation_id: u64) -> Vec<u8> {
        key_encoding::column_store_collection_prefix(relation_id)
    }

    pub(crate) fn column_store_row_prefix(relation_id: u64) -> Vec<u8> {
        key_encoding::column_store_row_prefix(relation_id)
    }

    pub(crate) fn column_store_row_key(relation_id: u64, id: &str) -> Vec<u8> {
        key_encoding::column_store_row_key(relation_id, id)
    }

    fn column_store_deleted_key(relation_id: u64, id: &str) -> Vec<u8> {
        key_encoding::column_store_deleted_key(relation_id, id)
    }

    pub(super) fn column_store_deleted_prefix(relation_id: u64) -> Vec<u8> {
        key_encoding::column_store_deleted_prefix(relation_id)
    }

    pub(super) fn column_store_field_prefix(relation_id: u64, field_id: u32) -> Vec<u8> {
        key_encoding::column_store_field_prefix(relation_id, field_id)
    }

    pub(super) fn column_store_field_key(relation_id: u64, field_id: u32, id: &str) -> Vec<u8> {
        key_encoding::column_store_field_key(relation_id, field_id, id)
    }
}

/// Keeps the row-type identities of existing metadata, else adopts the newly allocated pair.
fn assign_row_type_ids(
    metadata: &mut CollectionMeta,
    existing: Option<&CollectionMeta>,
    allocated: Option<(u64, u64)>,
) {
    match (existing, allocated) {
        (Some(existing), _) if existing.row_type_object_id != 0 => {
            metadata.row_type_object_id = existing.row_type_object_id;
            metadata.row_array_object_id = existing.row_array_object_id;
        }
        (_, Some((row_type, row_array))) => {
            metadata.row_type_object_id = row_type;
            metadata.row_array_object_id = row_array;
        }
        _ => {}
    }
}
