//! SQL-aware persisted index declaration and portable metadata operations.

use serde::{Deserialize, Serialize};

pub use cassie_core::catalog::declarations::IndexKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReservationIdentity<'a> { Legacy(&'a str), Generation(u64) }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniqueViolationDetails {
    pub table: String,
    pub column: String,
    pub constraint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexMeta {
    /// Durable expression contract: zero is legacy; one admits selected null-safe predicates.
    #[serde(
        default,
        skip_serializing_if = "crate::definition_guard::is_legacy"
    )]
    pub definition_semantics_version: u32,
    pub collection: String,
    pub name: String,
    pub field: String,
    #[serde(default)]
    pub fields: Vec<String>,
    #[serde(default)]
    pub expressions: Vec<String>,
    #[serde(default)]
    pub include_fields: Vec<String>,
    #[serde(default)]
    pub predicate: Option<String>,
    pub kind: IndexKind,
    pub unique: bool,
    pub options: std::collections::BTreeMap<String, String>,
}

impl IndexMeta {
    const STORAGE_ID_OPTION: &'static str = "__cassie_storage_id";
    const RELATION_ID_OPTION: &'static str = "__cassie_relation_id";
    const CATALOG_OID_OPTION: &'static str = "__cassie_catalog_oid";
    const UNIQUE_RESERVATION_NAME_OPTION: &'static str = "__cassie_unique_reservation_name";
    const UNIQUE_RESERVATION_GENERATION_OPTION: &'static str =
        "__cassie_unique_reservation_generation";
    /// Index option key that records the selected PostgreSQL access method.
    pub const ACCESS_METHOD_OPTION: &'static str = crate::index_options::ACCESS_METHOD_OPTION;
    pub const NULLS_NOT_DISTINCT_OPTION: &'static str = crate::index_options::NULLS_NOT_DISTINCT_OPTION;

    #[must_use]
    pub fn read_definition_matches(&self, other: &Self) -> bool {
        self.definition_semantics_version == other.definition_semantics_version
            && self.field == other.field
            && self.fields == other.fields
            && self.expressions == other.expressions
            && self.include_fields == other.include_fields
            && self.predicate == other.predicate
            && self.kind == other.kind
            && self.unique == other.unique
            && self
                .options
                .iter()
                .filter(|(key, _)| !Self::is_identity_option(key))
                .eq(other
                    .options
                    .iter()
                    .filter(|(key, _)| !Self::is_identity_option(key)))
    }

    fn is_identity_option(key: &str) -> bool {
        matches!(
            key,
            Self::STORAGE_ID_OPTION
                | Self::RELATION_ID_OPTION
                | Self::CATALOG_OID_OPTION
                | Self::UNIQUE_RESERVATION_NAME_OPTION
                | Self::UNIQUE_RESERVATION_GENERATION_OPTION
        )
    }

    #[must_use]
    pub fn clone_for_relation(&self, collection: &str, name: &str) -> Self {
        let mut cloned = self.clone();
        cloned.collection = collection.to_string();
        cloned.name = name.to_string();
        cloned.options.remove(Self::STORAGE_ID_OPTION);
        cloned.options.remove(Self::RELATION_ID_OPTION);
        cloned.options.remove(Self::CATALOG_OID_OPTION);
        cloned.options.remove(Self::UNIQUE_RESERVATION_NAME_OPTION);
        cloned
            .options
            .remove(Self::UNIQUE_RESERVATION_GENERATION_OPTION);
        cloned
    }

    #[must_use]
    pub fn nulls_not_distinct(&self) -> bool {
        self.options
            .get(Self::NULLS_NOT_DISTINCT_OPTION)
            .is_some_and(|value| value == "true")
    }

    /// Index option key for a per-key option such as `order` or `operator_class`.
    #[must_use]
    pub fn key_option_name(position: usize, option: &str) -> String {
        crate::index_options::key_option_name(position, option)
    }

    #[must_use]
    pub fn key_option(&self, position: usize, option: &str) -> Option<&str> {
        self.options
            .get(&Self::key_option_name(position, option))
            .map(String::as_str)
    }

    pub fn access_method(&self) -> &str {
        match &self.kind {
            IndexKind::Scalar => self
                .options
                .get(Self::ACCESS_METHOD_OPTION)
                .map_or("btree", String::as_str),
            IndexKind::FullText => "gin",
            IndexKind::Vector => self
                .options
                .get(Self::ACCESS_METHOD_OPTION)
                .map_or("vector", String::as_str),
            IndexKind::Hybrid => "btree",
            IndexKind::Column => "column",
            IndexKind::TimeSeries => "time_series",
        }
    }

    #[must_use]
    pub fn can_back_unique_constraint(&self) -> bool {
        if !self.unique
            || self.kind != IndexKind::Scalar
            || !self.access_method().eq_ignore_ascii_case("btree")
            || !self.expressions.is_empty()
            || self.predicate.is_some()
            || self.normalized_fields().is_empty()
        {
            return false;
        }
        (0..self.normalized_fields().len()).all(|position| {
            self.key_option(position, "order")
                .is_none_or(|value| value.eq_ignore_ascii_case("asc"))
                && self
                    .key_option(position, "nulls")
                    .is_none_or(|value| value.eq_ignore_ascii_case("last"))
        })
    }

    #[must_use]
    pub fn storage_id(&self) -> Option<u64> {
        self.options
            .get(Self::STORAGE_ID_OPTION)
            .and_then(|value| value.parse().ok())
    }

    #[must_use]
    pub fn catalog_oid(&self) -> Option<u32> {
        self.options
            .get(Self::CATALOG_OID_OPTION)
            .and_then(|value| value.parse().ok())
    }

    pub fn set_catalog_oid_if_missing(&mut self, oid: u32) {
        self.options
            .entry(Self::CATALOG_OID_OPTION.to_string())
            .or_insert_with(|| oid.to_string());
    }

    pub fn set_reservation_generation(&mut self, identity: u64) -> Result<(), String> {
        if identity == 0 || self.storage_id() != Some(identity) || self.relation_id().is_none_or(|id| id == 0) {
            return Err("invalid unique index reservation generation".to_owned());
        }
        self.options.insert(Self::UNIQUE_RESERVATION_GENERATION_OPTION.to_string(), identity.to_string());
        Ok(())
    }

    #[must_use]
    pub fn unique_reservation_name(&self) -> &str {
        self.options
            .get(Self::UNIQUE_RESERVATION_NAME_OPTION)
            .map_or(&self.name, String::as_str)
    }

    pub fn set_unique_reservation_name_if_missing(&mut self, name: &str) {
        self.options
            .entry(Self::UNIQUE_RESERVATION_NAME_OPTION.to_string())
            .or_insert_with(|| name.to_string());
    }

    #[must_use]
    pub fn relation_id(&self) -> Option<u64> {
        self.options
            .get(Self::RELATION_ID_OPTION)
            .and_then(|value| value.parse().ok())
    }

    pub fn set_storage_ids(&mut self, relation_id: u64, storage_id: u64) {
        self.options.insert(
            Self::RELATION_ID_OPTION.to_string(),
            relation_id.to_string(),
        );
        self.options
            .insert(Self::STORAGE_ID_OPTION.to_string(), storage_id.to_string());
    }

    pub fn clear_storage_ids(&mut self) {
        self.options.remove(Self::RELATION_ID_OPTION);
        self.options.remove(Self::STORAGE_ID_OPTION);
    }

    #[must_use]
    pub fn normalized_fields(&self) -> Vec<String> {
        if self.fields.is_empty() && !self.field.is_empty() {
            vec![self.field.clone()]
        } else {
            self.fields.clone()
        }
    }

    #[must_use]
    pub fn normalized_include_fields(&self) -> Vec<String> {
        self.include_fields.clone()
    }

    /// Whether dropping `field` invalidates this index: a key or INCLUDE column,
    /// a time-series partition column, or a vector index's embedding source.
    #[must_use]
    pub fn references_field(&self, field: &str) -> bool {
        let partition_references_field = self.options.get("partition_by").is_some_and(|fields| {
            fields.split(',').map(str::trim).any(|candidate| {
                crate::column_identifier::ColumnIdentifierPath::stored_field_key(candidate)
                    == crate::column_identifier::ColumnIdentifierPath::stored_field_key(field)
            })
        });
        let source_references_field = self.options.get("source_field").is_some_and(|source| {
            crate::column_identifier::ColumnIdentifierPath::stored_field_key(source)
                == crate::column_identifier::ColumnIdentifierPath::stored_field_key(field)
        });
        partition_references_field
            || source_references_field
            || self
                .normalized_fields()
                .iter()
                .chain(self.normalized_include_fields().iter())
                .any(|candidate| {
                    crate::column_identifier::ColumnIdentifierPath::stored_field_key(candidate)
                        == crate::column_identifier::ColumnIdentifierPath::stored_field_key(field)
                })
    }

    #[must_use]
    pub fn normalized_expressions(&self) -> Vec<String> {
        self.expressions.clone()
    }

    #[must_use]
    pub fn primary_field(&self) -> String {
        self.normalized_fields()
            .into_iter()
            .next()
            .unwrap_or_else(|| self.field.clone())
    }

    pub fn rename_field(&mut self, current: &str, next: &str) -> bool {
        let current_key = crate::column_identifier::ColumnIdentifierPath::stored_field_key(current);
        let rename_names = |names: &mut Vec<String>| {
            let mut changed = false;
            for name in names {
                if crate::column_identifier::ColumnIdentifierPath::stored_field_key(name) == current_key {
                    *name = next.to_string();
                    changed = true;
                }
            }
            changed
        };

        let mut fields = self.normalized_fields();
        let mut changed = rename_names(&mut fields);
        if changed {
            self.field = fields.first().cloned().unwrap_or_else(|| next.to_string());
            self.fields = fields;
        }
        changed |= rename_names(&mut self.include_fields);
        for option in ["partition_by", "source_field"] {
            if let Some(raw) = self.options.get_mut(option) {
                let mut names: Vec<String> =
                    raw.split(',').map(|part| part.trim().to_string()).collect();
                if rename_names(&mut names) {
                    *raw = names.join(",");
                    changed = true;
                }
            }
        }
        for expression in &mut self.expressions {
            changed |= Self::rename_expression_json(expression, &current_key, next);
        }
        if let Some(predicate) = self.predicate.as_mut() {
            changed |= Self::rename_expression_json(predicate, &current_key, next);
        }
        changed
    }

    /// Rewrites column references inside a serialized expression; text that
    /// does not parse as an expression is left as stored.
    fn rename_expression_json(raw: &mut String, current_key: &str, next: &str) -> bool {
        let Ok(mut expression) = serde_json::from_str::<crate::ast::Expr>(raw) else {
            return false;
        };
        expression.rename_column_references(current_key, next);
        match serde_json::to_string(&expression) {
            Ok(renamed) if renamed != *raw => {
                *raw = renamed;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{IndexKind, IndexMeta};
    use std::collections::BTreeMap;

    #[test]
    fn should_preserve_index_metadata_legacy_and_versioned_encoding() {
        // Arrange
        let mut index = IndexMeta {
            definition_semantics_version: 0,
            collection: "public.events".to_owned(),
            name: "events_key".to_owned(),
            field: "id".to_owned(),
            fields: vec!["id".to_owned()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: true,
            options: BTreeMap::new(),
        };

        // Act
        let legacy = serde_json::to_value(&index).expect("serialize legacy index metadata");
        index.definition_semantics_version = 1;
        let versioned = serde_json::to_value(&index).expect("serialize versioned index metadata");

        // Assert
        assert!(legacy.get("definition_semantics_version").is_none());
        assert_eq!(versioned["definition_semantics_version"], 1);
        assert_eq!(versioned["expressions"], serde_json::json!([]));
        assert_eq!(versioned["predicate"], serde_json::Value::Null);
    }
}

impl IndexMeta {
    #[must_use]
    pub fn reservation_identity_details(&self) -> Result<ReservationIdentity<'_>, String> {
        let Some(raw) = self.options.get(Self::UNIQUE_RESERVATION_GENERATION_OPTION) else {
            return Ok(ReservationIdentity::Legacy(self.unique_reservation_name()));
        };
        let identity = raw
            .parse::<u64>()
            .ok()
            .filter(|value| *value != 0)
            .filter(|value| {
                self.storage_id() == Some(*value) && self.relation_id().is_some_and(|id| id != 0)
            })
            .ok_or_else(|| "invalid unique index reservation generation".to_owned())?;
        Ok(ReservationIdentity::Generation(identity))
    }

    #[must_use]
    pub fn unique_violation_details(&self, collection: &str) -> UniqueViolationDetails {
        let column = if self.normalized_fields().is_empty() {
            self.normalized_expressions().join(", ")
        } else {
            self.normalized_fields().join(", ")
        };
        UniqueViolationDetails {
            table: collection.to_owned(),
            column,
            constraint: cassie_core::catalog::scope::local_name(&self.name),
        }
    }
}
