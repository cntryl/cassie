use serde::{Deserialize, Serialize};

use crate::types::DataType;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphMeta {
    pub name: String,
    pub node_collection: String,
    pub edge_collection: String,
    pub node_type_field: String,
    pub node_id_field: String,
    pub edge_id_field: String,
    pub source_type_field: String,
    pub source_id_field: String,
    pub target_type_field: String,
    pub target_id_field: String,
    pub edge_type_field: String,
    pub weight_field: String,
}

impl GraphMeta {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            node_collection: format!("{name}_nodes"),
            edge_collection: format!("{name}_edges"),
            node_type_field: "node_type".to_string(),
            node_id_field: "node_id".to_string(),
            edge_id_field: "edge_id".to_string(),
            source_type_field: "source_type".to_string(),
            source_id_field: "source_id".to_string(),
            target_type_field: "target_type".to_string(),
            target_id_field: "target_id".to_string(),
            edge_type_field: "edge_type".to_string(),
            weight_field: "weight".to_string(),
        }
    }

    pub fn node_builtin_fields(&self) -> Vec<(String, DataType)> {
        vec![
            (self.node_type_field.clone(), DataType::Text),
            (self.node_id_field.clone(), DataType::Text),
        ]
    }

    pub fn edge_builtin_fields(&self) -> Vec<(String, DataType)> {
        vec![
            (self.edge_id_field.clone(), DataType::Text),
            (self.source_type_field.clone(), DataType::Text),
            (self.source_id_field.clone(), DataType::Text),
            (self.target_type_field.clone(), DataType::Text),
            (self.target_id_field.clone(), DataType::Text),
            (self.edge_type_field.clone(), DataType::Text),
            (self.weight_field.clone(), DataType::Float),
        ]
    }
}
