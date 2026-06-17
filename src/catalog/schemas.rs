use crate::types::DataType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldMeta {
    pub name: String,
    pub data_type: DataType,
    pub is_indexed: bool,
    pub boost: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionSchema {
    pub collection: String,
    pub fields: Vec<FieldMeta>,
}

impl CollectionSchema {
    pub fn new(collection: impl Into<String>) -> Self {
        Self {
            collection: collection.into(),
            fields: Vec::new(),
        }
    }

    pub fn field(&self, name: &str) -> Option<&FieldMeta> {
        self.fields.iter().find(|f| f.name == name)
    }
}
