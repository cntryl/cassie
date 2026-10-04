use crate::embeddings::VectorIndexRecord;

use super::{Cassie, CassieError};

impl Cassie {
    pub(crate) fn apply_vector_indexes(
        &self,
        _collection: &str,
        payload: &mut serde_json::Value,
        indexes: &[VectorIndexRecord],
    ) -> Result<(), CassieError> {
        let object = payload.as_object_mut().ok_or_else(|| {
            CassieError::InvalidEmbedding("document payload must be a JSON object".to_string())
        })?;

        for index in indexes {
            self.validate_embedding_compatibility(index, None)?;

            // An embedding the caller supplied is stored as written; the provider only
            // derives a value for a row that arrives without one.
            if object
                .get(&index.field)
                .is_some_and(|value| !value.is_null())
            {
                continue;
            }

            // An omitted or NULL source column leaves the embedding absent.
            let Some(source_value) = object
                .get(&index.source_field)
                .filter(|value| !value.is_null())
            else {
                continue;
            };

            let embedding = self.embed_source_value(index, source_value)?;
            object.insert(index.field.clone(), embedding);
        }

        Ok(())
    }

    /// Derives the JSON embedding array a vector index stores for a source value.
    pub(super) fn embed_source_value(
        &self,
        index: &VectorIndexRecord,
        source_value: &serde_json::Value,
    ) -> Result<serde_json::Value, CassieError> {
        let source = source_value
            .as_str()
            .map_or_else(|| source_value.to_string(), str::to_string);
        let embedding = self
            .embedding_provider
            .embed_query(&source)
            .map_err(CassieError::from)?;
        Self::validate_embedding_payload(index, &embedding)?;
        Ok(serde_json::Value::Array(
            embedding
                .values
                .into_iter()
                .map(serde_json::Value::from)
                .collect(),
        ))
    }

    /// Drops derived embeddings that an UPDATE made stale.
    ///
    /// `payload` is the merged row. When `assigned` names a vector index's source
    /// column but not its embedding column, the carried-over embedding was derived
    /// from the previous source text, so it is removed and `apply_vector_indexes`
    /// derives a fresh one (or leaves it NULL when the source is now NULL).
    pub(crate) fn discard_stale_vector_embeddings<'a>(
        &self,
        collection: &str,
        payload: &mut serde_json::Value,
        assigned: impl IntoIterator<Item = &'a str>,
    ) {
        let indexes = self.catalog.list_vector_indexes(collection);
        let Some(object) = payload.as_object_mut() else {
            return;
        };
        let assigned = assigned.into_iter().collect::<Vec<_>>();
        let was_assigned = |name: &str| assigned.contains(&name);
        for index in &indexes {
            if was_assigned(&index.source_field) && !was_assigned(&index.field) {
                object.remove(&index.field);
            }
        }
    }
}
