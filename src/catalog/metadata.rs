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

    pub async fn clear(&self) {
        self.collections.write().await.clear();
        self.schemas.write().await.clear();
        self.vector_indexes.write().await.clear();
    }

    pub async fn unregister_collection(&self, collection: &str) {
        self.collections.write().await.remove(collection);

        self.schemas.write().await.remove(collection);

        self.vector_indexes
            .write()
            .await
            .retain(|_, index| index.collection != collection);
    }

    pub async fn get_schema(&self, collection: &str) -> Option<CollectionSchema> {
        let schemas = self.schemas.read().await;
        schemas.get(collection).cloned()
    }

    pub async fn add_collection_field(&self, collection: &str, name: String, data_type: DataType) {
        let mut schemas = self.schemas.write().await;
        let Some(schema) = schemas.get_mut(collection) else {
            return;
        };

        if schema.fields.iter().any(|field| field.name == name) {
            return;
        }

        schema.fields.push(crate::catalog::FieldMeta {
            name,
            data_type,
            is_indexed: true,
            boost: Some(1.0),
        });
    }

    pub async fn remove_collection_field(&self, collection: &str, name: &str) {
        let mut schemas = self.schemas.write().await;
        let Some(schema) = schemas.get_mut(collection) else {
            return;
        };

        schema.fields.retain(|field| field.name != name);
    }

    pub async fn rename_collection(&self, current_name: &str, next_name: &str) {
        let mut collections = self.collections.write().await;
        collections.remove(current_name);
        collections.insert(next_name.to_string(), CollectionMeta::new(next_name, None));

        let mut schemas = self.schemas.write().await;
        if let Some(schema) = schemas.remove(current_name) {
            schemas.insert(
                next_name.to_string(),
                CollectionSchema {
                    collection: next_name.to_string(),
                    fields: schema.fields,
                },
            );
        }

        let mut indexes = self.vector_indexes.write().await;
        let keys = indexes
            .iter()
            .filter(|(_, record)| record.collection == current_name)
            .map(|(key, record)| (key.clone(), record.field.clone()))
            .collect::<Vec<_>>();

        for (key, field) in keys {
            if let Some(mut metadata) = indexes.remove(&key) {
                metadata.collection = next_name.to_string();
                let next_key = Self::vector_index_key(&metadata.collection, &field);
                indexes.insert(next_key, metadata);
            }
        }
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

impl Default for Catalog {
    fn default() -> Self {
        Self::new()
    }
}
