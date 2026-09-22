use std::collections::HashSet;

use crate::app::{Cassie, CassieError, CatalogObjectKind};
use crate::catalog::{canonical_relation_name, CollectionMeta, DEFAULT_SCHEMA};
use crate::types::{DataType, FieldSchema, Schema};
use serde_json::Value;

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CreateCollectionRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub fields: Vec<FieldSpec>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FieldSpec {
    pub name: String,
    #[serde(rename = "type")]
    pub data_type: String,
}

#[must_use]
pub fn list(cassie: &Cassie) -> Vec<String> {
    cassie.midge.list_collections()
}

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn create(cassie: &Cassie, body: &[u8]) -> Result<Value, CassieError> {
    let request: CreateCollectionRequest =
        serde_json::from_slice(body).map_err(|e| CassieError::Parse(e.to_string()))?;

    let CreateCollectionRequest {
        name: raw_name,
        description,
        fields,
    } = request;
    let name = normalize_collection_name(&raw_name)?;
    if fields.is_empty() {
        return Err(CassieError::InvalidQuery(
            "collection definitions require at least one field".to_string(),
        ));
    }
    let mut schema_fields = Vec::new();
    let mut field_names = HashSet::new();
    for field in fields {
        let field_name = field.name.trim();
        if field_name.is_empty() {
            return Err(CassieError::InvalidQuery(
                "collection field names cannot be empty".to_string(),
            ));
        }
        if crate::types::row_identity::is_row_identity_column(field_name) {
            return Err(CassieError::InvalidQuery(
                "field '_id' conflicts with Cassie's reserved internal document identity"
                    .to_string(),
            ));
        }
        if !field_names.insert(field_name.to_ascii_lowercase()) {
            return Err(CassieError::Planner(format!(
                "collection field '{field_name}' is defined more than once"
            )));
        }
        let parsed = parse_data_type(field.data_type.as_str())?;
        schema_fields.push(FieldSchema {
            name: field_name.to_string(),
            data_type: parsed,
            nullable: true,
        });
    }

    let schema = Schema {
        fields: schema_fields,
    };
    let collection = canonical_relation_name(&cassie.default_database, DEFAULT_SCHEMA, &name);
    let metadata = CollectionMeta::new(&collection, description);
    cassie
        .midge
        .with_collection_gates(std::slice::from_ref(&collection), || {
            if cassie.catalog.relation_exists(&collection) {
                return Err(collection_already_exists(&collection));
            }

            let created = cassie.midge.create_collection_with_meta_if_absent(
                &collection,
                &schema,
                &metadata,
            )?;
            if !created {
                return Err(collection_already_exists(&collection));
            }

            cassie.catalog.register_collection_meta_with_constraints(
                metadata.clone(),
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
                Vec::new(),
            );
            Ok(())
        })?;
    cassie.bump_schema_epoch_and_invalidate_query_cache()?;

    Ok(serde_json::json!({
        "collection": name,
    }))
}

fn normalize_collection_name(raw: &str) -> Result<String, CassieError> {
    let name = raw.trim();
    let mut characters = name.chars();
    let valid_start = characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_');
    let valid_rest = characters
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '$'));
    if !valid_start || !valid_rest {
        return Err(CassieError::InvalidQuery(
            "collection names must be unqualified SQL identifiers".to_string(),
        ));
    }
    Ok(name.to_string())
}

fn collection_already_exists(name: &str) -> CassieError {
    CassieError::CatalogObjectAlreadyExists {
        kind: CatalogObjectKind::Relation,
        name: name.to_string(),
    }
}

fn parse_data_type(value: &str) -> Result<DataType, CassieError> {
    DataType::parse_sql(value).map_err(CassieError::Parse)
}
