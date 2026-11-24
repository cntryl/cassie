use super::super::WriteOptions;

/// Rejects a batch whose collections belong to more than one database.
pub(super) fn require_single_database<'a>(
    mut collections: impl Iterator<Item = &'a str>,
    database: &str,
) -> Result<(), crate::adapter::CassieError> {
    if collections.any(|collection| {
        crate::catalog::relation_database_name(collection)
            .is_some_and(|candidate| !candidate.eq_ignore_ascii_case(database))
    }) {
        return Err(crate::adapter::CassieError::Unsupported(
            "one transaction cannot access multiple databases".to_string(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct DocumentWriteBatchOptions {
    pub commit: WriteOptions,
    pub data_database: Option<String>,
    pub additional_data_writes: Vec<(Vec<u8>, Option<Vec<u8>>)>,
    pub rollup_change_fields: Vec<String>,
    pub publication_marker: Option<(Vec<u8>, Vec<u8>)>,
    pub refresh_after_commit: bool,
    pub normalized_vector_collection: Option<String>,
    pub record_rollup_maintenance_debt: bool,
    pub record_materialized_projection_maintenance_debt: bool,
    pub(super) replay: Option<super::super::projections::ReplayBatchCommit>,
}

impl DocumentWriteBatchOptions {
    /// True when the batch must commit even without row changes.
    pub(super) fn has_side_writes(&self) -> bool {
        self.replay.is_some() || !self.additional_data_writes.is_empty()
    }

    /// Stages the caller's extra Data writes in the batch transaction.
    pub(super) fn stage_additional_data_writes(
        &self,
        tx: &mut crate::adapter::AdapterTransaction,
    ) -> Result<(), crate::adapter::CassieError> {
        for (key, value) in &self.additional_data_writes {
            if let Some(value) = value {
                tx.put(key.clone(), value.clone(), None)
                    .map_err(crate::adapter::CassieError::from)?;
            } else {
                tx.delete(key.clone())
                    .map_err(crate::adapter::CassieError::from)?;
            }
        }
        Ok(())
    }

    /// The database a batch commits in: the explicit one, else the first
    /// collection's, else the default.
    pub(super) fn database_for(&self, first_collection: Option<&str>, default: &str) -> String {
        self.data_database
            .clone()
            .or_else(|| crate::catalog::relation_database_name(first_collection.unwrap_or("")))
            .unwrap_or_else(|| default.to_owned())
    }

    pub fn sync(commit: WriteOptions) -> Self {
        Self {
            commit,
            data_database: None,
            additional_data_writes: Vec::new(),
            rollup_change_fields: Vec::new(),
            publication_marker: None,
            refresh_after_commit: true,
            normalized_vector_collection: None,
            record_rollup_maintenance_debt: false,
            record_materialized_projection_maintenance_debt: false,
            replay: None,
        }
    }

    pub fn buffered(commit: WriteOptions) -> Self {
        Self {
            commit,
            data_database: None,
            additional_data_writes: Vec::new(),
            rollup_change_fields: Vec::new(),
            publication_marker: None,
            refresh_after_commit: true,
            normalized_vector_collection: None,
            record_rollup_maintenance_debt: false,
            record_materialized_projection_maintenance_debt: false,
            replay: None,
        }
    }
}
