use std::env;
use std::path::Path;
use std::sync::OnceLock;

use cntryl_midge::{ColumnFamilyHandle, Engine, Query, TransactionMode, WriteOptions};
use uuid::Uuid;

use crate::app::CassieError;
use crate::types::{DataType, FieldSchema, Schema, Value, Vector};

fn allow_memory_fallback() -> bool {
    env::var("CASSIE_MIDGE_ALLOW_FALLBACK")
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

const SCHEMA_FAMILY_NAME: &str = "cf0";
const DATA_FAMILY_NAME: &str = "cf1";
const TEMP_FAMILY_NAME: &str = "cf2";
const DEFAULT_FAMILY_NAME: &str = "default";
const VECTOR_INDEX_PREFIX: &str = "__cassie__/vector-index/";
const SCHEMA_COLLECTION_KEY_PREFIX: &str = "__cassie__/schema/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageFamily {
    Schema,
    Data,
    Temp,
}

impl StorageFamily {
    fn name(self) -> &'static str {
        match self {
            Self::Schema => SCHEMA_FAMILY_NAME,
            Self::Data => DATA_FAMILY_NAME,
            Self::Temp => TEMP_FAMILY_NAME,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StorageLayout {
    pub schema: ColumnFamilyHandle,
    pub data: ColumnFamilyHandle,
    pub temp: ColumnFamilyHandle,
}

pub struct Midge {
    engine: Engine,
    storage_layout: OnceLock<StorageLayout>,
}

#[derive(Debug, Clone)]
pub struct DocumentRef {
    pub id: String,
    pub payload: serde_json::Value,
}

impl Midge {
    pub fn new() -> Result<Self, CassieError> {
        let data_dir =
            env::var("CASSIE_MIDGE_DATA_DIR").unwrap_or_else(|_| "./.cassie/midge".to_string());
        Self::new_with_data_dir(data_dir)
    }

    pub fn new_with_data_dir(data_dir: impl AsRef<Path>) -> Result<Self, CassieError> {
        let options = cntryl_midge::OpenOptions::local(data_dir.as_ref()).build();

        let engine = match Engine::open(options) {
            Ok(engine) => engine,
            Err(error) => {
                if allow_memory_fallback() {
                    Engine::open(cntryl_midge::OpenOptions::in_memory().build())
                        .map_err(CassieError::from)?
                } else {
                    return Err(CassieError::from(error));
                }
            }
        };

        Ok(Self {
            engine,
            storage_layout: OnceLock::new(),
        })
    }

    pub fn bootstrap_families(&self) -> Result<StorageLayout, CassieError> {
        let schema = self.get_or_create_family(StorageFamily::Schema)?;
        let data = self.get_or_create_family(StorageFamily::Data)?;
        let temp = self.get_or_create_family(StorageFamily::Temp)?;

        if schema.id() == data.id() || schema.id() == temp.id() || data.id() == temp.id() {
            return Err(CassieError::StorageBootstrap(
                "family ids must be distinct for schema/data/temp families".to_string(),
            ));
        }

        Ok(StorageLayout { schema, data, temp })
    }

    pub fn ensure_families_ready(&self) -> Result<&StorageLayout, CassieError> {
        if self.storage_layout.get().is_none() {
            let layout = self.bootstrap_families()?;
            let _ = self.storage_layout.set(layout);
        }

        self.storage_layout.get().ok_or_else(|| {
            CassieError::StorageBootstrap("failed to initialize midge storage families".to_string())
        })
    }

    pub fn storage_layout(&self) -> Option<StorageLayout> {
        self.storage_layout.get().cloned()
    }

    pub fn schema_tx(
        &self,
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        self.transaction(StorageFamily::Schema, mode)
    }

    pub fn data_tx(&self, mode: TransactionMode) -> Result<cntryl_midge::Transaction, CassieError> {
        self.transaction(StorageFamily::Data, mode)
    }

    pub fn temp_tx(&self, mode: TransactionMode) -> Result<cntryl_midge::Transaction, CassieError> {
        self.transaction(StorageFamily::Temp, mode)
    }

    pub fn default_tx(
        &self,
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        self.transaction_by_name(DEFAULT_FAMILY_NAME, mode)
    }

    fn transaction(
        &self,
        family: StorageFamily,
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        let layout = self.ensure_families_ready()?;
        let cf = match family {
            StorageFamily::Schema => &layout.schema,
            StorageFamily::Data => &layout.data,
            StorageFamily::Temp => &layout.temp,
        };

        self.engine
            .begin_tx(cf.id(), mode)
            .map_err(CassieError::from)
    }

    fn transaction_by_name(
        &self,
        family: &str,
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        let Some(cf) = self.engine.get_column_family(family) else {
            return Err(CassieError::StorageMissingFamily(format!(
                "required column family '{family}' is missing"
            )));
        };

        self.engine
            .begin_tx(cf.id(), mode)
            .map_err(CassieError::from)
    }

    fn get_or_create_family(
        &self,
        family: StorageFamily,
    ) -> Result<ColumnFamilyHandle, CassieError> {
        let name = family.name();
        if let Some(existing) = self.engine.get_column_family(name) {
            return Ok(existing);
        }

        if let Ok(created) = self.engine.create_column_family(name) {
            return Ok(created);
        }

        self.engine.get_column_family(name).ok_or_else(|| {
            CassieError::StorageBootstrap(format!("cannot resolve required column family '{name}'"))
        })
    }

    fn collection_schema_key(collection: &str) -> Vec<u8> {
        format!("{SCHEMA_COLLECTION_KEY_PREFIX}{collection}").into_bytes()
    }

    fn schema_collection_prefix() -> Vec<u8> {
        SCHEMA_COLLECTION_KEY_PREFIX.as_bytes().to_vec()
    }

    fn vector_index_key(collection: &str, field: &str) -> Vec<u8> {
        format!("{VECTOR_INDEX_PREFIX}{collection}/{field}").into_bytes()
    }

    fn vector_index_prefix() -> Vec<u8> {
        VECTOR_INDEX_PREFIX.as_bytes().to_vec()
    }

    fn collections_key() -> Vec<u8> {
        b"__cassie__/collections".to_vec()
    }

    fn doc_prefix(collection: &str) -> Vec<u8> {
        format!("doc:{collection}:").into_bytes()
    }

    fn doc_key(collection: &str, id: &str) -> Vec<u8> {
        format!("doc:{collection}:{id}").into_bytes()
    }

    fn begin_schema_readonly_tx(&self) -> Result<cntryl_midge::Transaction, CassieError> {
        self.schema_tx(TransactionMode::ReadOnly)
    }

    fn begin_schema_rw_tx(&self) -> Result<cntryl_midge::Transaction, CassieError> {
        self.schema_tx(TransactionMode::ReadWrite)
    }

    fn begin_data_readonly_tx(&self) -> Result<cntryl_midge::Transaction, CassieError> {
        self.data_tx(TransactionMode::ReadOnly)
    }

    fn begin_data_rw_tx(&self) -> Result<cntryl_midge::Transaction, CassieError> {
        self.data_tx(TransactionMode::ReadWrite)
    }

    pub async fn raw_get(
        &self,
        family: StorageFamily,
        key: &[u8],
    ) -> Result<Option<Vec<u8>>, CassieError> {
        let tx = self.transaction(family, TransactionMode::ReadOnly)?;
        let value = tx.get(key).map_err(CassieError::from)?;
        Ok(value.map(|value| value.to_vec()))
    }

    pub async fn raw_scan_prefix(
        &self,
        family: StorageFamily,
        prefix: &[u8],
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, CassieError> {
        let tx = self.transaction(family, TransactionMode::ReadOnly)?;
        let mut iterator = tx
            .scan(&Query::new().prefix(prefix.to_vec().into()))
            .map_err(CassieError::from)?;

        let mut values = Vec::new();
        while let Some((key, value)) = iterator.next() {
            values.push((key, value));
        }
        Ok(values)
    }

    pub async fn raw_scan_prefix_named(
        &self,
        family: &str,
        prefix: &[u8],
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, CassieError> {
        let tx = self.transaction_by_name(family, TransactionMode::ReadOnly)?;
        let mut iterator = tx
            .scan(&Query::new().prefix(prefix.to_vec().into()))
            .map_err(CassieError::from)?;

        let mut values = Vec::new();
        while let Some((key, value)) = iterator.next() {
            values.push((key, value));
        }
        Ok(values)
    }

    pub async fn clear_temp_family(&self) -> Result<usize, CassieError> {
        let mut tx = self.temp_tx(TransactionMode::ReadWrite)?;
        let mut iterator = tx.scan(&Query::new()).map_err(CassieError::from)?;
        let mut keys = Vec::new();
        while let Some((raw_key, _)) = iterator.next() {
            keys.push(raw_key);
        }

        if keys.is_empty() {
            return Ok(0);
        }

        let deleted = keys.len();
        for key in keys {
            tx.delete(key).map_err(CassieError::from)?;
        }

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(deleted)
    }

    async fn load_collections(
        &self,
        tx: &cntryl_midge::Transaction,
    ) -> Result<Vec<String>, CassieError> {
        let raw = tx
            .get(&Self::collections_key())
            .map_err(CassieError::from)?;
        if raw.is_none() {
            return Ok(Vec::new());
        }
        let parsed: Vec<String> = serde_json::from_slice(&raw.unwrap())
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        Ok(parsed)
    }

    async fn save_collections(
        &self,
        tx: &mut cntryl_midge::Transaction,
        collections: &[String],
    ) -> Result<(), CassieError> {
        let value = serde_json::to_vec(collections)
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::collections_key(), value, None)
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub async fn create_collection(&self, name: &str, schema: Schema) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;

        let schema_key = Self::collection_schema_key(name);
        if tx.get(&schema_key).map_err(CassieError::from)?.is_none() {
            let schema_bytes = serde_json::to_vec(&schema)
                .map_err(|error| CassieError::Parse(error.to_string()))?;
            tx.put(schema_key, schema_bytes, None)
                .map_err(CassieError::from)?;
        }

        let mut collections = self.load_collections(&tx).await?;
        if !collections.iter().any(|entry| entry == name) {
            collections.push(name.to_string());
            collections.sort();
            self.save_collections(&mut tx, &collections).await?;
        }

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    pub async fn put_vector_index(
        &self,
        metadata: crate::embeddings::VectorIndexRecord,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let key = Self::vector_index_key(&metadata.collection, &metadata.field);

        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key, value, None).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub async fn get_vector_index(
        &self,
        collection: &str,
        field: &str,
    ) -> Result<Option<crate::embeddings::VectorIndexRecord>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;

        let raw = tx
            .get(&Self::vector_index_key(collection, field))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid vector index metadata: {error}")))
    }

    pub async fn list_vector_indexes(
        &self,
    ) -> Result<Vec<crate::embeddings::VectorIndexRecord>, CassieError> {
        let entries = self
            .raw_scan_prefix(StorageFamily::Schema, &Self::vector_index_prefix())
            .await?;
        let mut out = Vec::with_capacity(entries.len());

        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }

        Ok(out)
    }

    pub async fn collection_schema(&self, name: &str) -> Option<Schema> {
        let tx = self.begin_schema_readonly_tx().ok()?;
        let raw = tx.get(&Self::collection_schema_key(name)).ok()??;
        serde_json::from_slice(&raw).ok()
    }

    pub async fn list_collections(&self) -> Vec<String> {
        let tx = match self.begin_schema_readonly_tx() {
            Ok(tx) => tx,
            Err(_) => return Vec::new(),
        };

        self.load_collections(&tx)
            .await
            .map(|mut values| {
                values.sort();
                values
            })
            .unwrap_or_else(|_| Vec::new())
    }

    pub async fn list_collections_from_schema(&self) -> Vec<String> {
        let tx = match self.begin_schema_readonly_tx() {
            Ok(tx) => tx,
            Err(_) => return Vec::new(),
        };
        let Ok(mut scan) = tx.scan(&Query::new().prefix(Self::schema_collection_prefix().into()))
        else {
            return Vec::new();
        };

        let mut collections = Vec::new();
        while let Some((raw_key, _raw_value)) = scan.next() {
            let key = String::from_utf8(raw_key).unwrap_or_default();
            let name = key
                .strip_prefix(SCHEMA_COLLECTION_KEY_PREFIX)
                .unwrap_or("")
                .to_string();
            if !name.is_empty() {
                collections.push(name);
            }
        }

        collections.sort();
        collections.dedup();
        collections
    }

    pub async fn put_document(
        &self,
        collection: &str,
        id: Option<String>,
        payload: serde_json::Value,
    ) -> Result<String, CassieError> {
        let schema = self
            .collection_schema(collection)
            .await
            .ok_or_else(|| CassieError::CollectionNotFound(collection.to_string()))?;

        Self::validate_document(&schema, &payload)?;

        let doc_id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let mut tx = self.begin_data_rw_tx()?;
        tx.put(
            Self::doc_key(collection, &doc_id),
            payload.to_string().into_bytes(),
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(doc_id)
    }

    pub async fn get_document(
        &self,
        collection: &str,
        id: &str,
    ) -> Result<Option<DocumentRef>, CassieError> {
        if self.collection_schema(collection).await.is_none() {
            return Err(CassieError::CollectionNotFound(collection.to_string()));
        }

        let tx = self.begin_data_readonly_tx()?;
        let payload = tx
            .get(&Self::doc_key(collection, id))
            .map_err(CassieError::from)?;

        let Some(payload) = payload else {
            return Ok(None);
        };
        let payload = serde_json::from_slice(&payload)
            .map_err(|error| CassieError::Parse(error.to_string()))?;

        Ok(Some(DocumentRef {
            id: id.to_string(),
            payload,
        }))
    }

    pub async fn delete_document(&self, collection: &str, id: &str) -> Result<bool, CassieError> {
        if self.collection_schema(collection).await.is_none() {
            return Err(CassieError::CollectionNotFound(collection.to_string()));
        }

        let key = Self::doc_key(collection, id);
        let mut tx = self.begin_data_rw_tx()?;
        if tx.get(&key).map_err(CassieError::from)?.is_some() {
            tx.delete(key).map_err(CassieError::from)?;
            tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
            return Ok(true);
        }

        tx.rollback().map_err(CassieError::from)?;
        Ok(false)
    }

    pub async fn scan_documents(&self, collection: &str) -> Result<Vec<DocumentRef>, CassieError> {
        if self.collection_schema(collection).await.is_none() {
            return Err(CassieError::CollectionNotFound(collection.to_string()));
        }

        let tx = self.begin_data_readonly_tx()?;
        let mut iter = tx
            .scan(&Query::new().prefix(Self::doc_prefix(collection).into()))
            .map_err(CassieError::from)?;
        let needle = format!("doc:{collection}:");
        let mut results = Vec::new();

        while let Some((raw_key, raw_value)) = iter.next() {
            let raw_key = String::from_utf8(raw_key).map_err(|error| {
                CassieError::Parse(format!("invalid document key in storage: {error}"))
            })?;
            let id = raw_key.strip_prefix(&needle).unwrap_or("").to_string();
            if id.is_empty() {
                continue;
            }

            let payload = serde_json::from_slice(&raw_value).map_err(|error| {
                CassieError::Parse(format!("invalid document payload: {error}"))
            })?;
            results.push(DocumentRef { id, payload });
        }

        Ok(results)
    }

    pub async fn all_fields_json(
        &self,
        collection: &str,
    ) -> Result<Vec<(String, serde_json::Value)>, CassieError> {
        self.scan_documents(collection)
            .await
            .map(|docs| docs.into_iter().map(|doc| (doc.id, doc.payload)).collect())
    }

    fn validate_document(schema: &Schema, payload: &serde_json::Value) -> Result<(), CassieError> {
        let map = payload
            .as_object()
            .ok_or_else(|| CassieError::InvalidVector("document must be object".to_string()))?;

        for field in &schema.fields {
            if let Some(value) = map.get(&field.name) {
                if let DataType::Vector(dim) = field.data_type {
                    if let Some(arr) = value.as_array() {
                        if arr.len() != dim {
                            return Err(CassieError::InvalidVector(format!(
                                "field '{}' expects vector({}) but got {}",
                                field.name,
                                dim,
                                arr.len()
                            )));
                        }
                    } else {
                        return Err(CassieError::InvalidVector(format!(
                            "field '{}' expects vector({}) but received non-array",
                            field.name, dim
                        )));
                    }
                }
            }
        }
        Ok(())
    }
}

impl From<&Value> for Vector {
    fn from(value: &Value) -> Self {
        match value {
            Value::Vector(v) => v.clone(),
            _ => Vector::new(Vec::new()),
        }
    }
}

pub fn vector_from_json(value: &serde_json::Value) -> Option<Vector> {
    let arr = value.as_array()?;
    let mut nums = Vec::with_capacity(arr.len());
    for n in arr {
        nums.push(n.as_f64()? as f32);
    }
    Some(Vector::new(nums))
}

#[allow(dead_code)]
pub fn field_schema(name: &str, data_type: DataType) -> FieldSchema {
    FieldSchema {
        name: name.to_string(),
        data_type,
        nullable: true,
    }
}
