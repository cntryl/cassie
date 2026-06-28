use super::*;

impl Cassie {
    pub fn register_collection(&self, name: impl Into<String>, schema: crate::types::Schema) {
        let name = name.into();
        self.catalog.register_collection(
            &name,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
        self.invalidate_plan_cache();
    }

    pub fn register_vector_index(&self, index: VectorIndexRecord) {
        self.catalog.register_vector_index(index);
        self.invalidate_plan_cache();
    }

    pub(crate) fn invalidate_plan_cache(&self) {
        self.runtime.invalidate_plan_cache();
    }

    pub(crate) fn bump_schema_epoch_and_invalidate_query_cache(&self) -> Result<(), CassieError> {
        let schema_epoch = self
            .midge
            .bump_schema_epoch()
            .map_err(|error| CassieError::Storage(format!("bump schema epoch: {error}")))?;
        self.runtime.record_storage_access("schema", true, true);
        self.midge
            .clear_runtime_feedback_records()
            .map_err(|error| CassieError::Storage(format!("clear operator feedback: {error}")))?;
        self.runtime.record_storage_access("schema", true, true);
        self.runtime.set_schema_epoch(schema_epoch);
        self.runtime.invalidate_plan_cache();
        self.run_deferred_schema_cleanup()?;
        Ok(())
    }
}
