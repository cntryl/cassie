use crate::app::Cassie;
use crate::app::CassieError;
use crate::types::{DataType, FieldSchema, Schema};
use serde_json::Value;

#[derive(serde::Deserialize)]
pub struct CreateCollectionRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub fields: Vec<FieldSpec>,
}

#[derive(serde::Deserialize)]
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

    let mut schema_fields = Vec::new();
    for field in request.fields {
        let parsed = parse_data_type(field.data_type.as_str())?;
        schema_fields.push(FieldSchema {
            name: field.name,
            data_type: parsed,
            nullable: true,
        });
    }

    let schema = Schema {
        fields: schema_fields,
    };

    cassie
        .midge
        .create_collection(&request.name, schema.clone())?;

    cassie.catalog.register_collection(
        &request.name,
        schema
            .fields
            .iter()
            .map(|f| (f.name.clone(), f.data_type.clone()))
            .collect(),
    );

    Ok(serde_json::json!({
        "collection": request.name,
    }))
}

fn parse_data_type(value: &str) -> Result<DataType, CassieError> {
    DataType::parse_sql(value).map_err(CassieError::Parse)
}
