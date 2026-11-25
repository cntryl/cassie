//! Full-text field metadata: per-field boosts and indexed text fields.
use super::Catalog;
use crate::types::DataType;

impl Catalog {
    #[must_use]
    pub fn get_field_boost(&self, collection: &str, field: &str) -> Option<f32> {
        let schemas = self.schemas.read();
        schemas
            .get(collection)
            .and_then(|schema| schema.field(field))
            .and_then(|field| field.boost)
    }

    #[must_use]
    pub fn set_field_boost(&self, collection: &str, field: &str, boost: f32) -> bool {
        let mut schemas = self.schemas.write();
        let Some(schema) = schemas.get_mut(collection) else {
            return false;
        };

        let Some(field_meta) = schema.fields.iter_mut().find(|entry| entry.name == field) else {
            return false;
        };

        field_meta.boost = Some(boost);
        self.bump_version();
        true
    }

    #[must_use]
    pub fn text_fields(&self, collection: &str) -> Vec<String> {
        let schemas = self.schemas.read();
        schemas.get(collection).map_or_default(|schema| {
            schema
                .fields
                .iter()
                .filter(|field| field.is_indexed && field.data_type == DataType::Text)
                .map(|field| field.name.clone())
                .collect()
        })
    }
}
