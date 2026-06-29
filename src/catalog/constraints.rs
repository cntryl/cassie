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
    pub primary_key_name: Option<String>,
    #[serde(default)]
    pub primary_key_ordinal: Option<u32>,
    #[serde(default)]
    pub unique_name: Option<String>,
    #[serde(default)]
    pub unique_ordinal: Option<u32>,
    #[serde(default)]
    pub check_name: Option<String>,
    #[serde(default)]
    pub foreign_key_name: Option<String>,
    #[serde(default)]
    pub foreign_key_ordinal: Option<u32>,
    #[serde(default)]
    pub foreign_key_on_delete: Option<String>,
    #[serde(default)]
    pub foreign_key_on_update: Option<String>,
    #[serde(default)]
    pub not_null: bool,
    #[serde(default)]
    pub unique: bool,
    #[serde(default)]
    pub primary_key: bool,
    #[serde(default)]
    pub default_value: Option<serde_json::Value>,
    #[serde(default)]
    pub default_expression: Option<String>,
    #[serde(default)]
    pub default_sequence: Option<String>,
    #[serde(default)]
    pub default_sequence_owned: bool,
    #[serde(default)]
    pub check: Option<ConstraintCheck>,
    #[serde(default)]
    pub references_table: Option<String>,
    #[serde(default)]
    pub references_field: Option<String>,
}

impl FieldConstraint {
    pub fn new(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            primary_key_name: None,
            primary_key_ordinal: None,
            unique_name: None,
            unique_ordinal: None,
            check_name: None,
            foreign_key_name: None,
            foreign_key_ordinal: None,
            foreign_key_on_delete: None,
            foreign_key_on_update: None,
            not_null: false,
            unique: false,
            primary_key: false,
            default_value: None,
            default_expression: None,
            default_sequence: None,
            default_sequence_owned: false,
            check: None,
            references_table: None,
            references_field: None,
        }
    }
}

#[must_use]
pub fn generated_constraint_name(collection: &str, field: &str, kind: &str) -> String {
    format!(
        "{}_{}_{}",
        collection,
        field,
        kind.to_ascii_lowercase().replace(' ', "_")
    )
}

pub fn merge_constraint_set(
    existing: &mut Vec<FieldConstraint>,
    additions: impl IntoIterator<Item = FieldConstraint>,
) {
    for addition in additions {
        if !is_constraint_populated(&addition) {
            continue;
        }
        if let Some(current) = existing
            .iter_mut()
            .find(|entry| entry.field.eq_ignore_ascii_case(&addition.field))
        {
            merge_constraint(current, addition);
        } else {
            existing.push(addition);
        }
    }
}

fn is_constraint_populated(constraint: &FieldConstraint) -> bool {
    constraint.primary_key
        || constraint.unique
        || constraint.not_null
        || constraint.default_value.is_some()
        || constraint.default_expression.is_some()
        || constraint.default_sequence.is_some()
        || constraint.check.is_some()
        || constraint.references_table.is_some()
}

fn merge_constraint(existing: &mut FieldConstraint, next: FieldConstraint) {
    existing.not_null |= next.not_null;
    existing.unique |= next.unique;
    existing.primary_key |= next.primary_key;
    if next.primary_key_name.is_some() {
        existing.primary_key_name = next.primary_key_name;
    }
    if next.primary_key_ordinal.is_some() {
        existing.primary_key_ordinal = next.primary_key_ordinal;
    }
    if next.unique_name.is_some() {
        existing.unique_name = next.unique_name;
    }
    if next.unique_ordinal.is_some() {
        existing.unique_ordinal = next.unique_ordinal;
    }
    if next.default_value.is_some() {
        existing.default_value = next.default_value;
    }
    if next.default_expression.is_some() {
        existing.default_expression = next.default_expression;
    }
    if next.default_sequence.is_some() {
        existing.default_sequence = next.default_sequence;
    }
    existing.default_sequence_owned |= next.default_sequence_owned;
    if next.check.is_some() {
        existing.check = next.check;
    }
    if next.check_name.is_some() {
        existing.check_name = next.check_name;
    }
    if next.references_table.is_some() {
        existing.references_table = next.references_table;
        existing.references_field = next.references_field;
    }
    if next.foreign_key_name.is_some() {
        existing.foreign_key_name = next.foreign_key_name;
    }
    if next.foreign_key_ordinal.is_some() {
        existing.foreign_key_ordinal = next.foreign_key_ordinal;
    }
    if next.foreign_key_on_delete.is_some() {
        existing.foreign_key_on_delete = next.foreign_key_on_delete;
    }
    if next.foreign_key_on_update.is_some() {
        existing.foreign_key_on_update = next.foreign_key_on_update;
    }
}
