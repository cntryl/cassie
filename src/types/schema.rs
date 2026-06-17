use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DataType {
    Int,
    Float,
    Boolean,
    Text,
    Vector(usize),
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldSchema {
    pub name: String,
    pub data_type: DataType,
    pub nullable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schema {
    pub fields: Vec<FieldSchema>,
}

impl Schema {
    pub fn vector_fields(&self) -> Vec<&FieldSchema> {
        self.fields
            .iter()
            .filter(|f| matches!(f.data_type, DataType::Vector(_)))
            .collect()
    }
}

impl std::iter::FromIterator<(String, DataType)> for Schema {
    fn from_iter<T: IntoIterator<Item = (String, DataType)>>(iter: T) -> Self {
        Self {
            fields: iter
                .into_iter()
                .map(|(name, data_type)| FieldSchema {
                    name,
                    data_type,
                    nullable: true,
                })
                .collect(),
        }
    }
}

impl Schema {
    pub fn field(&self, name: &str) -> Option<&FieldSchema> {
        self.fields.iter().find(|f| f.name == name)
    }
}
