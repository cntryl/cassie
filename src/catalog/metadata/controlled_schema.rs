use std::mem::size_of;

use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::{
    name_matches, view_schema_to_collection_schema, CassieError, Catalog, CollectionSchema,
    DataType, FieldMeta,
};

impl Catalog {
    pub(crate) fn contains_declared_stored_field(&self, collection: &str, field: &str) -> bool {
        let schemas = self.schemas.read();
        schemas
            .get(collection)
            .or_else(|| {
                schemas
                    .iter()
                    .find(|(stored, _)| name_matches(stored, collection))
                    .map(|(_, schema)| schema)
            })
            .is_some_and(|schema| schema.fields.iter().any(|entry| entry.name == field))
    }

    pub(crate) fn clone_schema_with_controls(
        &self,
        collection: &str,
        controls: &QueryExecutionControls,
    ) -> Result<(Option<CollectionSchema>, Option<QueryMemoryReservation>), CassieError> {
        let schemas = self.schemas.read();
        if let Some(schema) = schemas.get(collection).or_else(|| {
            schemas
                .iter()
                .find(|(stored, _)| name_matches(stored, collection))
                .map(|(_, schema)| schema)
        }) {
            let bytes = schema_bytes(
                &schema.collection,
                schema.fields.len(),
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.as_str(), &field.data_type)),
            )?;
            let memory = controls.reserve_query_memory(bytes)?;
            return Ok((Some(schema.clone()), Some(memory)));
        }
        drop(schemas);
        let views = self.views.read();
        let Some(view) = views.get(collection).or_else(|| {
            views
                .iter()
                .find(|(stored, _)| name_matches(stored, collection))
                .map(|(_, view)| view)
        }) else {
            return Ok((None, None));
        };
        let bytes = schema_bytes(
            &view.name,
            view.schema.fields.len(),
            view.schema
                .fields
                .iter()
                .map(|field| (field.name.as_str(), &field.data_type)),
        )?;
        let memory = controls.reserve_query_memory(bytes)?;
        Ok((
            Some(view_schema_to_collection_schema(&view.name, &view.schema)),
            Some(memory),
        ))
    }
}

fn schema_bytes<'a>(
    collection: &str,
    field_count: usize,
    mut fields: impl Iterator<Item = (&'a str, &'a DataType)>,
) -> Result<usize, CassieError> {
    let inline = field_count
        .checked_mul(size_of::<FieldMeta>())
        .and_then(|bytes| bytes.checked_add(size_of::<CollectionSchema>()))
        .and_then(|bytes| bytes.checked_add(collection.len()))
        .ok_or_else(overflow)?;
    fields.try_fold(inline, |bytes, (name, data_type)| {
        let type_bytes = data_type_heap_bytes(data_type)?;
        bytes
            .checked_add(name.len())
            .and_then(|bytes| bytes.checked_add(type_bytes))
            .ok_or_else(overflow)
    })
}

fn data_type_heap_bytes(data_type: &DataType) -> Result<usize, CassieError> {
    match data_type {
        DataType::Array(inner) => size_of::<DataType>()
            .checked_add(data_type_heap_bytes(inner)?)
            .ok_or_else(overflow),
        _ => Ok(0),
    }
}

fn overflow() -> CassieError {
    CassieError::ResourceLimit("controlled catalog schema accounting overflow".to_owned())
}
