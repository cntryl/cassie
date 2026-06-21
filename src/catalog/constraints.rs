use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ConstraintOperator {
    Eq,
    NotEq,
    Lt,
    Lte,
    Gt,
    Gte,
    Like,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConstraintCheck {
    pub field: String,
    pub operator: ConstraintOperator,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FieldConstraint {
    pub field: String,
    #[serde(default)]
    pub not_null: bool,
    #[serde(default)]
    pub unique: bool,
    #[serde(default)]
    pub primary_key: bool,
    #[serde(default)]
    pub default_value: Option<serde_json::Value>,
    #[serde(default)]
    pub check: Option<ConstraintCheck>,
    #[serde(default)]
    pub references_table: Option<String>,
    #[serde(default)]
    pub references_field: Option<String>,
}

pub fn generated_constraint_name(collection: &str, field: &str, kind: &str) -> String {
    format!(
        "{}_{}_{}",
        collection,
        field,
        kind.to_ascii_lowercase().replace(' ', "_")
    )
}
