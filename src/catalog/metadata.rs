use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::catalog::{CollectionMeta, CollectionSchema};
use crate::embeddings::VectorIndexRecord;
use crate::types::DataType;

#[derive(Debug, Clone)]
pub struct Catalog {
    pub collections: Arc<RwLock<HashMap<String, CollectionMeta>>>,
    pub schemas: Arc<RwLock<HashMap<String, CollectionSchema>>>,
    pub vector_indexes: Arc<RwLock<HashMap<String, VectorIndexRecord>>>,
}

impl Catalog {
    pub fn new() -> Self {
        Self {
            collections: Arc::new(RwLock::new(HashMap::new())),
            schemas: Arc::new(RwLock::new(HashMap::new())),
            vector_indexes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register_collection(&self, name: &str, schema: Vec<(String, DataType)>) {
        let mut collections = self.collections.write().await;
        collections.insert(name.to_string(), CollectionMeta::new(name, None));

        let mut schemas = self.schemas.write().await;
        let fields = schema
            .into_iter()
            .map(|(name, data_type)| crate::catalog::FieldMeta {
                name,
                data_type,
                is_indexed: true,
                boost: Some(1.0),
            })
            .collect();
        schemas.insert(
            name.to_string(),
            CollectionSchema {
                collection: name.to_string(),
                fields,
            },
        );
    }

    pub async fn list_collections(&self) -> Vec<CollectionMeta> {
        let collections = self.collections.read().await;
        collections.values().cloned().collect()
    }

    pub async fn get_schema(&self, collection: &str) -> Option<CollectionSchema> {
        let schemas = self.schemas.read().await;
        schemas.get(collection).cloned()
    }

    pub async fn exists(&self, collection: &str) -> bool {
        self.collections.read().await.contains_key(collection)
    }

    pub async fn get_field_boost(&self, collection: &str, field: &str) -> Option<f32> {
        let schemas = self.schemas.read().await;
        schemas
            .get(collection)
            .and_then(|schema| schema.field(field))
            .and_then(|field| field.boost)
    }

    pub async fn set_field_boost(&self, collection: &str, field: &str, boost: f32) -> bool {
        let mut schemas = self.schemas.write().await;
        let Some(schema) = schemas.get_mut(collection) else {
            return false;
        };

        let Some(field_meta) = schema.fields.iter_mut().find(|entry| entry.name == field) else {
            return false;
        };

        field_meta.boost = Some(boost);
        true
    }

    pub async fn text_fields(&self, collection: &str) -> Vec<String> {
        let schemas = self.schemas.read().await;
        schemas
            .get(collection)
            .map(|schema| {
                schema
                    .fields
                    .iter()
                    .filter(|field| field.is_indexed && field.data_type == DataType::Text)
                    .map(|field| field.name.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub async fn register_vector_index(&self, record: VectorIndexRecord) {
        let mut indexes = self.vector_indexes.write().await;
        let key = Self::vector_index_key(&record.collection, &record.field);
        indexes.insert(key, record);
    }

    pub async fn get_vector_index(
        &self,
        collection: &str,
        vector_field: &str,
    ) -> Option<VectorIndexRecord> {
        let indexes = self.vector_indexes.read().await;
        indexes
            .get(&Self::vector_index_key(collection, vector_field))
            .cloned()
    }

    pub async fn list_vector_indexes(&self, collection: &str) -> Vec<VectorIndexRecord> {
        let indexes = self.vector_indexes.read().await;
        indexes
            .values()
            .filter(|record| record.collection == collection)
            .cloned()
            .collect()
    }

    pub async fn clear_vector_indexes(&self, collection: &str) {
        let mut indexes = self.vector_indexes.write().await;
        indexes.retain(|_, value| value.collection != collection);
    }

    fn vector_index_key(collection: &str, field: &str) -> String {
        format!("{collection}:{field}")
    }
}
