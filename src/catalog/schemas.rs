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

    #[must_use]
    pub fn field(&self, name: &str) -> Option<&FieldMeta> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Whether this schema declares its own `id` column, which shadows the
    /// legacy `id` alias for the internal row identity (see
    /// [`crate::types::row_identity`]).
    #[must_use]
    pub fn declares_id(&self) -> bool {
        crate::types::row_identity::declares_id(self.fields.iter().map(|field| field.name.as_str()))
    }
}
