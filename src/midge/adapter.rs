use std::collections::HashSet;
use std::env;
use std::path::Path;
use std::sync::OnceLock;

use cntryl_midge::{ColumnFamilyHandle, Engine, Query, TransactionMode, WriteOptions};
use uuid::Uuid;

use crate::app::CassieError;
use crate::catalog::{FieldConstraint, IndexMeta, NamespaceMeta, RoleMeta};
use crate::midge::row_blob::{decode_projected_row, decode_row, encode_row, RowSchema};
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
const INDEX_PREFIX: &str = "__cassie__/index/";
const CONSTRAINTS_PREFIX: &str = "__cassie__/constraints/";
const FUNCTION_PREFIX: &str = "__cassie__/function/";
const PROCEDURE_PREFIX: &str = "__cassie__/procedure/";
const VIEW_PREFIX: &str = "__cassie__/view/";
const ROLE_PREFIX: &str = "__cassie__/role/";
const SCHEMA_COLLECTION_KEY_PREFIX: &str = "__cassie__/schema/";
const ROW_SCHEMA_KEY_PREFIX: &str = "__cassie__/row-schema/";
const SCHEMA_NAMESPACE_KEY_PREFIX: &str = "__cassie__/namespace/";
const NAMESPACES_KEY: &str = "__cassie__/namespaces";

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

#[derive(Debug, Clone, Copy)]
struct FamilyScope {
    include_schema: bool,
    include_data: bool,
    include_temp: bool,
}

impl FamilyScope {
    fn for_families(families: &[StorageFamily]) -> Result<Self, CassieError> {
        if families.is_empty() {
            return Err(CassieError::Unsupported(
                "transaction scope must include at least one storage family".to_string(),
            ));
        }

        let include_schema = families
            .iter()
            .any(|family| matches!(family, StorageFamily::Schema));
        let include_data = families
            .iter()
            .any(|family| matches!(family, StorageFamily::Data));
        let include_temp = families
            .iter()
            .any(|family| matches!(family, StorageFamily::Temp));

        if include_schema && include_data {
            return Err(CassieError::Unsupported(
                "cannot open a transaction across schema and data families".to_string(),
            ));
        }

        if include_temp && (include_schema || include_data) {
            return Err(CassieError::Unsupported(
                "transactions currently support exactly one storage family".to_string(),
            ));
        }

        Ok(Self {
            include_schema,
            include_data,
            include_temp,
        })
    }

    fn family(self) -> Option<StorageFamily> {
        if self.include_schema {
            Some(StorageFamily::Schema)
        } else if self.include_data {
            Some(StorageFamily::Data)
        } else if self.include_temp {
            Some(StorageFamily::Temp)
        } else {
            None
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowDecode {
    Full,
    Projected(Vec<String>),
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
        self.begin_families_tx(&[StorageFamily::Schema], mode)
    }

    pub fn data_tx(&self, mode: TransactionMode) -> Result<cntryl_midge::Transaction, CassieError> {
        self.begin_families_tx(&[StorageFamily::Data], mode)
    }

    pub fn temp_tx(&self, mode: TransactionMode) -> Result<cntryl_midge::Transaction, CassieError> {
        self.begin_families_tx(&[StorageFamily::Temp], mode)
    }

    pub fn default_tx(
        &self,
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        self.transaction_by_name(DEFAULT_FAMILY_NAME, mode)
    }

    pub fn begin_families_tx(
        &self,
        families: &[StorageFamily],
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        let scope = FamilyScope::for_families(families)?;
        let family = scope.family().ok_or_else(|| {
            CassieError::Unsupported(
                "transactions currently support exactly one storage family".to_string(),
            )
        })?;

        self.transaction(family, mode)
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

    fn row_schema_key(collection: &str) -> Vec<u8> {
        format!("{ROW_SCHEMA_KEY_PREFIX}{collection}").into_bytes()
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

    fn vector_index_collection_prefix(collection: &str) -> Vec<u8> {
        format!("{VECTOR_INDEX_PREFIX}{collection}/").into_bytes()
    }

    fn index_key(collection: &str, name: &str) -> Vec<u8> {
        format!("{INDEX_PREFIX}{collection}/{name}").into_bytes()
    }

    fn index_prefix() -> Vec<u8> {
        INDEX_PREFIX.as_bytes().to_vec()
    }

    fn index_collection_prefix(collection: &str) -> Vec<u8> {
        format!("{INDEX_PREFIX}{collection}/").into_bytes()
    }

    fn function_key(name: &str) -> Vec<u8> {
        format!("{FUNCTION_PREFIX}{}", name.to_ascii_lowercase()).into_bytes()
    }

    fn function_prefix() -> Vec<u8> {
        FUNCTION_PREFIX.as_bytes().to_vec()
    }

    fn procedure_key(name: &str) -> Vec<u8> {
        format!("{PROCEDURE_PREFIX}{}", name.to_ascii_lowercase()).into_bytes()
    }

    fn procedure_prefix() -> Vec<u8> {
        PROCEDURE_PREFIX.as_bytes().to_vec()
    }

    fn view_key(name: &str) -> Vec<u8> {
        format!("{VIEW_PREFIX}{name}").into_bytes()
    }

    fn view_prefix() -> Vec<u8> {
        VIEW_PREFIX.as_bytes().to_vec()
    }

    fn role_key(name: &str) -> Vec<u8> {
        format!("{ROLE_PREFIX}{}", name.to_ascii_lowercase()).into_bytes()
    }

    fn role_prefix() -> Vec<u8> {
        ROLE_PREFIX.as_bytes().to_vec()
    }

    fn constraints_key(collection: &str) -> Vec<u8> {
        format!("{CONSTRAINTS_PREFIX}{collection}").into_bytes()
    }

    fn namespace_key(namespace: &str) -> Vec<u8> {
        format!("{SCHEMA_NAMESPACE_KEY_PREFIX}{namespace}").into_bytes()
    }

    fn namespace_prefix() -> Vec<u8> {
        SCHEMA_NAMESPACE_KEY_PREFIX.as_bytes().to_vec()
    }

    fn namespaces_key() -> Vec<u8> {
        NAMESPACES_KEY.as_bytes().to_vec()
    }

    fn collections_key() -> Vec<u8> {
        b"__cassie__/collections".to_vec()
    }

    fn row_prefix(collection: &str) -> Vec<u8> {
        format!("r/{collection}/").into_bytes()
    }

    fn row_key(collection: &str, id: &str) -> Vec<u8> {
        format!("r/{collection}/{id}").into_bytes()
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

    async fn load_namespaces(
        &self,
        tx: &cntryl_midge::Transaction,
    ) -> Result<Vec<String>, CassieError> {
        let raw = tx.get(&Self::namespaces_key()).map_err(CassieError::from)?;
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

    async fn save_namespaces(
        &self,
        tx: &mut cntryl_midge::Transaction,
        namespaces: &[String],
    ) -> Result<(), CassieError> {
        let value = serde_json::to_vec(namespaces)
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::namespaces_key(), value, None)
            .map_err(CassieError::from)?;
        Ok(())
    }

    fn load_row_schema_from_tx(
        tx: &cntryl_midge::Transaction,
        collection: &str,
    ) -> Result<Option<RowSchema>, CassieError> {
        let raw = tx
            .get(&Self::row_schema_key(collection))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        serde_json::from_slice(&raw).map(Some).map_err(|error| {
            CassieError::Parse(format!("invalid row schema for '{collection}': {error}"))
        })
    }

    fn save_row_schema_to_tx(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        row_schema: &RowSchema,
    ) -> Result<(), CassieError> {
        let value = serde_json::to_vec(row_schema)
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::row_schema_key(collection), value, None)
            .map_err(CassieError::from)?;
        Ok(())
    }

    async fn row_schema(&self, collection: &str) -> Result<RowSchema, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        if let Some(row_schema) = Self::load_row_schema_from_tx(&tx, collection)? {
            return Ok(row_schema);
        }

        let raw = tx
            .get(&Self::collection_schema_key(collection))
            .map_err(CassieError::from)?
            .ok_or_else(|| CassieError::CollectionNotFound(collection.to_string()))?;
        let schema: Schema = serde_json::from_slice(&raw).map_err(|error| {
            CassieError::Parse(format!("invalid schema for '{collection}': {error}"))
        })?;
        Ok(RowSchema::from_schema(&schema))
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
        if tx
            .get(&Self::row_schema_key(name))
            .map_err(CassieError::from)?
            .is_none()
        {
            Self::save_row_schema_to_tx(&mut tx, name, &RowSchema::from_schema(&schema))?;
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

    pub async fn create_namespace(&self, namespace: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;

        let namespace_key = Self::namespace_key(namespace);
        if tx.get(&namespace_key).map_err(CassieError::from)?.is_none() {
            let metadata = NamespaceMeta::new(namespace, None);
            let serialized = serde_json::to_vec(&metadata)
                .map_err(|error| CassieError::Parse(error.to_string()))?;
            tx.put(namespace_key, serialized, None)
                .map_err(CassieError::from)?;
        }

        let mut namespaces = self.load_namespaces(&tx).await?;
        if !namespaces.iter().any(|entry| entry == namespace) {
            namespaces.push(namespace.to_string());
            namespaces.sort();
            self.save_namespaces(&mut tx, &namespaces).await?;
        }

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    pub async fn list_namespaces(&self) -> Vec<String> {
        let tx = match self.begin_schema_readonly_tx() {
            Ok(tx) => tx,
            Err(_) => return Vec::new(),
        };

        if let Ok(namespaces) = self.load_namespaces(&tx).await {
            if !namespaces.is_empty() {
                let mut namespaces = namespaces;
                namespaces.sort();
                namespaces.dedup();
                return namespaces;
            }
        }

        let Ok(mut scan) = tx.scan(&Query::new().prefix(Self::namespace_prefix().into())) else {
            return Vec::new();
        };

        let mut namespaces = Vec::new();
        while let Some((raw_key, _raw_value)) = scan.next() {
            let key = String::from_utf8(raw_key).unwrap_or_default();
            let name = key
                .strip_prefix(SCHEMA_NAMESPACE_KEY_PREFIX)
                .unwrap_or("")
                .to_string();
            if !name.is_empty() {
                namespaces.push(name);
            }
        }

        namespaces.sort();
        namespaces.dedup();
        namespaces
    }

    pub async fn drop_collection(&self, name: &str) -> Result<(), CassieError> {
        let mut schema_tx = self.begin_schema_rw_tx()?;
        let schema_key = Self::collection_schema_key(name);
        if schema_tx
            .get(&schema_key)
            .map_err(CassieError::from)?
            .is_none()
        {
            return Err(CassieError::CollectionNotFound(name.to_string()));
        }

        let vector_prefix = Self::vector_index_collection_prefix(name);
        let mut vector_indexes = schema_tx
            .scan(&Query::new().prefix(vector_prefix.into()))
            .map_err(CassieError::from)?;
        let mut vector_keys = Vec::new();
        while let Some((key, _value)) = vector_indexes.next() {
            vector_keys.push(key);
        }
        for key in vector_keys {
            schema_tx.delete(key).map_err(CassieError::from)?;
        }

        let index_prefix = Self::index_collection_prefix(name);
        let mut index_scan = schema_tx
            .scan(&Query::new().prefix(index_prefix.into()))
            .map_err(CassieError::from)?;
        let mut index_keys = Vec::new();
        while let Some((key, _)) = index_scan.next() {
            index_keys.push(key);
        }
        for key in index_keys {
            schema_tx.delete(key).map_err(CassieError::from)?;
        }

        schema_tx
            .delete(Self::constraints_key(name))
            .map_err(CassieError::from)?;
        schema_tx
            .delete(Self::row_schema_key(name))
            .map_err(CassieError::from)?;

        let mut collections = self.load_collections(&schema_tx).await?;
        collections.retain(|entry| entry != name);
        self.save_collections(&mut schema_tx, &collections).await?;
        schema_tx.delete(schema_key).map_err(CassieError::from)?;
        schema_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;

        let mut data_tx = self.begin_data_rw_tx()?;
        let mut document_keys = Vec::new();
        for data_prefix in [Self::row_prefix(name), Self::doc_prefix(name)] {
            let mut documents = data_tx
                .scan(&Query::new().prefix(data_prefix.into()))
                .map_err(CassieError::from)?;
            while let Some((key, _value)) = documents.next() {
                document_keys.push(key);
            }
        }

        for key in document_keys {
            data_tx.delete(key).map_err(CassieError::from)?;
        }
        data_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;

        Ok(())
    }

    pub async fn alter_collection_add_column(
        &self,
        collection: &str,
        field: FieldSchema,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let schema_key = Self::collection_schema_key(collection);
        let schema_raw = tx.get(&schema_key).map_err(CassieError::from)?;
        let Some(schema_raw) = schema_raw else {
            return Err(CassieError::CollectionNotFound(collection.to_string()));
        };

        let mut schema: Schema = serde_json::from_slice(&schema_raw).map_err(|error| {
            CassieError::Parse(format!("invalid schema for '{collection}': {error}"))
        })?;

        if schema.fields.iter().any(|entry| entry.name == field.name) {
            return Err(CassieError::Unsupported(format!(
                "field '{0}' already exists on collection '{collection}'",
                field.name
            )));
        }

        let mut row_schema = Self::load_row_schema_from_tx(&tx, collection)?
            .unwrap_or_else(|| RowSchema::from_schema(&schema));
        row_schema.add_field(field.clone())?;
        Self::save_row_schema_to_tx(&mut tx, collection, &row_schema)?;

        schema.fields.push(field);
        let schema_bytes =
            serde_json::to_vec(&schema).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(schema_key, schema_bytes, None)
            .map_err(CassieError::from)?;

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    pub async fn alter_collection_drop_column(
        &self,
        collection: &str,
        field: &str,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let schema_key = Self::collection_schema_key(collection);
        let schema_raw = tx.get(&schema_key).map_err(CassieError::from)?;
        let Some(schema_raw) = schema_raw else {
            return Err(CassieError::CollectionNotFound(collection.to_string()));
        };

        let mut schema: Schema = serde_json::from_slice(&schema_raw).map_err(|error| {
            CassieError::Parse(format!("invalid schema for '{collection}': {error}"))
        })?;
        let original_schema = schema.clone();

        let field_count_before = schema.fields.len();
        schema.fields.retain(|entry| entry.name != field);
        if schema.fields.len() == field_count_before {
            return Err(CassieError::Unsupported(format!(
                "field '{field}' not found in collection '{collection}'",
            )));
        }

        let mut row_schema = Self::load_row_schema_from_tx(&tx, collection)?
            .unwrap_or_else(|| RowSchema::from_schema(&original_schema));
        if !row_schema.retire_field(field) {
            return Err(CassieError::Unsupported(format!(
                "field '{field}' not found in collection '{collection}'",
            )));
        }
        Self::save_row_schema_to_tx(&mut tx, collection, &row_schema)?;

        let schema_bytes =
            serde_json::to_vec(&schema).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(schema_key, schema_bytes, None)
            .map_err(CassieError::from)?;

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    pub async fn rename_collection(
        &self,
        current_name: &str,
        next_name: &str,
    ) -> Result<(), CassieError> {
        let mut schema_tx = self.begin_schema_rw_tx()?;

        let current_schema_key = Self::collection_schema_key(current_name);
        let current_schema_bytes = schema_tx
            .get(&current_schema_key)
            .map_err(CassieError::from)?
            .ok_or_else(|| CassieError::CollectionNotFound(current_name.to_string()))?;

        let next_schema_key = Self::collection_schema_key(next_name);
        if schema_tx
            .get(&next_schema_key)
            .map_err(CassieError::from)?
            .is_some()
        {
            return Err(CassieError::Unsupported(format!(
                "collection '{next_name}' already exists"
            )));
        }

        schema_tx
            .delete(current_schema_key)
            .map_err(CassieError::from)?;
        schema_tx
            .put(next_schema_key, current_schema_bytes.to_vec(), None)
            .map_err(CassieError::from)?;

        let current_row_schema_key = Self::row_schema_key(current_name);
        if let Some(row_schema_bytes) = schema_tx
            .get(&current_row_schema_key)
            .map_err(CassieError::from)?
        {
            schema_tx
                .delete(current_row_schema_key)
                .map_err(CassieError::from)?;
            schema_tx
                .put(
                    Self::row_schema_key(next_name),
                    row_schema_bytes.to_vec(),
                    None,
                )
                .map_err(CassieError::from)?;
        }

        let mut collections = self.load_collections(&schema_tx).await?;
        if let Some(position) = collections.iter().position(|entry| entry == current_name) {
            collections[position] = next_name.to_string();
            collections.sort();
            collections.dedup();
            self.save_collections(&mut schema_tx, &collections).await?;
        }

        let vector_prefix = Self::vector_index_collection_prefix(current_name);
        let mut vector_indexes = schema_tx
            .scan(&Query::new().prefix(vector_prefix.into()))
            .map_err(CassieError::from)?;
        let mut vector_keys = Vec::new();
        while let Some((key, _value)) = vector_indexes.next() {
            vector_keys.push(key);
        }

        for key in vector_keys {
            let Some(raw_value) = schema_tx.get(&key).map_err(CassieError::from)? else {
                continue;
            };
            let Ok(mut record) =
                serde_json::from_slice::<crate::embeddings::VectorIndexRecord>(&raw_value)
            else {
                continue;
            };

            record.collection = next_name.to_string();
            schema_tx.delete(key).map_err(CassieError::from)?;
            let next_key = Self::vector_index_key(&record.collection, &record.field);
            let value = serde_json::to_vec(&record)
                .map_err(|error| CassieError::Parse(error.to_string()))?;
            schema_tx
                .put(next_key, value, None)
                .map_err(CassieError::from)?;
        }

        let index_prefix = Self::index_collection_prefix(current_name);
        let mut indexes = schema_tx
            .scan(&Query::new().prefix(index_prefix.into()))
            .map_err(CassieError::from)?;
        let mut index_keys = Vec::new();
        while let Some((key, _value)) = indexes.next() {
            index_keys.push(key);
        }
        for key in index_keys {
            let Some(raw_value) = schema_tx.get(&key).map_err(CassieError::from)? else {
                continue;
            };
            let Ok(mut metadata) = serde_json::from_slice::<IndexMeta>(&raw_value) else {
                continue;
            };

            metadata.collection = next_name.to_string();
            schema_tx.delete(key).map_err(CassieError::from)?;
            let next_key = Self::index_key(&metadata.collection, &metadata.name);
            let value = serde_json::to_vec(&metadata)
                .map_err(|error| CassieError::Parse(error.to_string()))?;
            schema_tx
                .put(next_key, value, None)
                .map_err(CassieError::from)?;
        }

        let current_constraints_key = Self::constraints_key(current_name);
        let constraints = schema_tx
            .get(&current_constraints_key)
            .map_err(CassieError::from)?;
        if let Some(raw) = constraints {
            schema_tx
                .delete(current_constraints_key)
                .map_err(CassieError::from)?;
            schema_tx
                .put(Self::constraints_key(next_name), raw.to_vec(), None)
                .map_err(CassieError::from)?;
        }

        schema_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;

        let mut data_tx = self.begin_data_rw_tx()?;
        for (current_prefix, next_prefix) in [
            (Self::row_prefix(current_name), Self::row_prefix(next_name)),
            (Self::doc_prefix(current_name), Self::doc_prefix(next_name)),
        ] {
            let mut documents = data_tx
                .scan(&Query::new().prefix(current_prefix.clone().into()))
                .map_err(CassieError::from)?;
            let mut entries = Vec::new();
            while let Some((key, value)) = documents.next() {
                entries.push((key, value));
            }

            for (key, value) in entries {
                if let Some(id) = key.strip_prefix(current_prefix.as_slice()) {
                    let next_key = [next_prefix.as_slice(), id].concat();
                    data_tx.delete(key).map_err(CassieError::from)?;
                    data_tx
                        .put(next_key, value, None)
                        .map_err(CassieError::from)?;
                }
            }
        }
        data_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;

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

    pub async fn put_index(&self, metadata: IndexMeta) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let key = Self::index_key(&metadata.collection, &metadata.name);
        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key, value, None).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub async fn get_index(
        &self,
        collection: &str,
        name: &str,
    ) -> Result<Option<IndexMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let raw = tx
            .get(&Self::index_key(collection, name))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid index metadata: {error}")))
    }

    pub async fn list_indexes(&self) -> Result<Vec<IndexMeta>, CassieError> {
        let entries = self
            .raw_scan_prefix(StorageFamily::Schema, &Self::index_prefix())
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

    pub async fn delete_index(&self, collection: &str, name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::index_key(collection, name))
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub async fn delete_vector_index(
        &self,
        collection: &str,
        field: &str,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::vector_index_key(collection, field))
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub async fn save_constraints(
        &self,
        collection: &str,
        constraints: &[FieldConstraint],
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let value = serde_json::to_vec(constraints)
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::constraints_key(collection), value, None)
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub async fn load_constraints(
        &self,
        collection: &str,
    ) -> Result<Vec<FieldConstraint>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let raw = tx
            .get(&Self::constraints_key(collection))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(Vec::new());
        };

        serde_json::from_slice(&raw)
            .map_err(|error| CassieError::Parse(format!("invalid constraint metadata: {error}")))
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

    pub async fn put_function(
        &self,
        metadata: crate::catalog::FunctionMeta,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let key = Self::function_key(&metadata.name);
        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key, value, None).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub async fn get_function(
        &self,
        name: &str,
    ) -> Result<Option<crate::catalog::FunctionMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let raw = tx
            .get(&Self::function_key(name))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid function metadata: {error}")))
    }

    pub async fn list_functions(&self) -> Result<Vec<crate::catalog::FunctionMeta>, CassieError> {
        let entries = self
            .raw_scan_prefix(StorageFamily::Schema, &Self::function_prefix())
            .await?;
        let mut out: Vec<crate::catalog::FunctionMeta> = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        out.sort_by_key(|metadata| metadata.name.to_ascii_lowercase());
        Ok(out)
    }

    pub async fn delete_function(&self, name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::function_key(name))
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub async fn put_procedure(
        &self,
        metadata: crate::catalog::ProcedureMeta,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let key = Self::procedure_key(&metadata.name);
        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key, value, None).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub async fn get_procedure(
        &self,
        name: &str,
    ) -> Result<Option<crate::catalog::ProcedureMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let raw = tx
            .get(&Self::procedure_key(name))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid procedure metadata: {error}")))
    }

    pub async fn list_procedures(&self) -> Result<Vec<crate::catalog::ProcedureMeta>, CassieError> {
        let entries = self
            .raw_scan_prefix(StorageFamily::Schema, &Self::procedure_prefix())
            .await?;
        let mut out: Vec<crate::catalog::ProcedureMeta> = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        out.sort_by_key(|metadata| metadata.name.to_ascii_lowercase());
        Ok(out)
    }

    pub async fn delete_procedure(&self, name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::procedure_key(name))
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub async fn put_view(&self, metadata: crate::catalog::ViewMeta) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let key = Self::view_key(&metadata.name);
        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key, value, None).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub async fn get_view(
        &self,
        name: &str,
    ) -> Result<Option<crate::catalog::ViewMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let raw = tx.get(&Self::view_key(name)).map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid view metadata: {error}")))
    }

    pub async fn list_views(&self) -> Result<Vec<crate::catalog::ViewMeta>, CassieError> {
        let entries = self
            .raw_scan_prefix(StorageFamily::Schema, &Self::view_prefix())
            .await?;
        let mut out: Vec<crate::catalog::ViewMeta> = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        out.sort_by_key(|metadata| metadata.name.to_ascii_lowercase());
        Ok(out)
    }

    pub async fn delete_view(&self, name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::view_key(name)).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub async fn put_role(&self, metadata: RoleMeta) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let key = Self::role_key(&metadata.name);
        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key, value, None).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub async fn get_role(&self, name: &str) -> Result<Option<RoleMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let raw = tx.get(&Self::role_key(name)).map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid role metadata: {error}")))
    }

    pub async fn list_roles(&self) -> Result<Vec<RoleMeta>, CassieError> {
        let entries = self
            .raw_scan_prefix(StorageFamily::Schema, &Self::role_prefix())
            .await?;
        let mut out: Vec<RoleMeta> = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        out.sort_by_key(|metadata| metadata.name.to_ascii_lowercase());
        Ok(out)
    }

    pub async fn delete_role(&self, name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::role_key(name)).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub async fn collection_schema(&self, name: &str) -> Option<Schema> {
        let tx = self.begin_schema_readonly_tx().ok()?;
        if let Ok(Some(row_schema)) = Self::load_row_schema_from_tx(&tx, name) {
            return Some(row_schema.active_schema());
        }
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
        let row_schema = self.row_schema(collection).await?;

        Self::validate_document(&schema, &payload)?;
        let row_blob = encode_row(&row_schema, &payload)?;

        let doc_id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let mut tx = self.begin_data_rw_tx()?;
        tx.put(Self::row_key(collection, &doc_id), row_blob, None)
            .map_err(CassieError::from)?;
        let legacy_key = Self::doc_key(collection, &doc_id);
        if tx.get(&legacy_key).map_err(CassieError::from)?.is_some() {
            tx.delete(legacy_key).map_err(CassieError::from)?;
        }
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(doc_id)
    }

    pub async fn get_document(
        &self,
        collection: &str,
        id: &str,
    ) -> Result<Option<DocumentRef>, CassieError> {
        let row_schema = self.row_schema(collection).await?;

        let tx = self.begin_data_readonly_tx()?;
        let payload = match tx
            .get(&Self::row_key(collection, id))
            .map_err(CassieError::from)?
        {
            Some(payload) => Some(payload),
            None => tx
                .get(&Self::doc_key(collection, id))
                .map_err(CassieError::from)?,
        };

        let Some(payload) = payload else {
            return Ok(None);
        };
        let payload = decode_row(&row_schema, &payload)?;

        Ok(Some(DocumentRef {
            id: id.to_string(),
            payload,
        }))
    }

    pub async fn delete_document(&self, collection: &str, id: &str) -> Result<bool, CassieError> {
        let _row_schema = self.row_schema(collection).await?;

        let key = Self::row_key(collection, id);
        let legacy_key = Self::doc_key(collection, id);
        let mut tx = self.begin_data_rw_tx()?;
        let row_exists = tx.get(&key).map_err(CassieError::from)?.is_some();
        let legacy_exists = tx.get(&legacy_key).map_err(CassieError::from)?.is_some();
        if row_exists {
            tx.delete(key).map_err(CassieError::from)?;
        }
        if legacy_exists {
            tx.delete(legacy_key).map_err(CassieError::from)?;
        }
        if row_exists || legacy_exists {
            tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
            return Ok(true);
        }

        tx.rollback().map_err(CassieError::from)?;
        Ok(false)
    }

    pub async fn scan_documents_batched(
        &self,
        collection: &str,
        batch_size: usize,
    ) -> Result<Vec<Vec<DocumentRef>>, CassieError> {
        self.scan_rows_batched(collection, batch_size, RowDecode::Full)
            .await
    }

    pub async fn scan_rows_for_rebuild(
        &self,
        collection: &str,
        decode: RowDecode,
    ) -> Result<Vec<DocumentRef>, CassieError> {
        self.scan_rows_batched(collection, 1024, decode)
            .await
            .map(|batches| batches.into_iter().flatten().collect())
    }

    async fn scan_rows_batched(
        &self,
        collection: &str,
        batch_size: usize,
        decode: RowDecode,
    ) -> Result<Vec<Vec<DocumentRef>>, CassieError> {
        let row_schema = self.row_schema(collection).await?;
        let projection = match decode {
            RowDecode::Full => None,
            RowDecode::Projected(fields) => Some(
                fields
                    .into_iter()
                    .map(|field| field.to_ascii_lowercase())
                    .collect::<HashSet<_>>(),
            ),
        };

        let tx = self.begin_data_readonly_tx()?;
        let batch_size = batch_size.max(1);
        let mut results = Vec::new();
        let mut current = Vec::with_capacity(batch_size);
        let mut seen_ids = HashSet::new();

        for (prefix, needle, include_seen) in [
            (
                Self::row_prefix(collection),
                format!("r/{collection}/"),
                true,
            ),
            (
                Self::doc_prefix(collection),
                format!("doc:{collection}:"),
                false,
            ),
        ] {
            let mut iter = tx
                .scan(&Query::new().prefix(prefix.into()))
                .map_err(CassieError::from)?;
            while let Some((raw_key, raw_value)) = iter.next() {
                let raw_key = String::from_utf8(raw_key).map_err(|error| {
                    CassieError::Parse(format!("invalid document key in storage: {error}"))
                })?;
                let id = raw_key.strip_prefix(&needle).unwrap_or("").to_string();
                if id.is_empty() || (!include_seen && seen_ids.contains(&id)) {
                    continue;
                }
                seen_ids.insert(id.clone());

                let payload = match projection.as_ref() {
                    Some(projection) => decode_projected_row(&row_schema, &raw_value, projection)?,
                    None => decode_row(&row_schema, &raw_value)?,
                };
                current.push(DocumentRef { id, payload });
                if current.len() >= batch_size {
                    results.push(current);
                    current = Vec::with_capacity(batch_size);
                }
            }
        }

        if !current.is_empty() {
            results.push(current);
        }

        Ok(results)
    }

    pub async fn scan_documents(&self, collection: &str) -> Result<Vec<DocumentRef>, CassieError> {
        self.scan_documents_batched(collection, 1024)
            .await
            .map(|batches| batches.into_iter().flatten().collect())
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
