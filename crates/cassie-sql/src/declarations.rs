//! SQL declarations whose payload includes parsed expressions.

use serde::{Deserialize, Serialize};

use crate::ast::Expr;

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
    IsDistinctFrom,
    IsNotDistinctFrom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = ""))]
pub struct ConstraintCheck<A = ()> {
    pub field: String,
    pub operator: ConstraintOperator,
    pub value: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound_expression: Option<Box<Expr<A>>>,
}

impl<A: std::fmt::Debug> PartialEq for ConstraintCheck<A> {
    fn eq(&self, other: &Self) -> bool {
        self.field == other.field
            && self.operator == other.operator
            && self.value == other.value
            && match (&self.bound_expression, &other.bound_expression) {
                (None, None) => true,
                (Some(left), Some(right)) => left.structurally_eq(right),
                _ => false,
            }
    }
}

impl<A> ConstraintCheck<A> {
    /// Follows a column rename in the checked field and its bound expression.
    pub fn rename_field(&mut self, current: &str, next: &str) -> bool {
        let current_key = crate::column_identifier::ColumnIdentifierPath::stored_field_key(current);
        if crate::column_identifier::ColumnIdentifierPath::stored_field_key(&self.field)
            != current_key
        {
            return false;
        }
        self.field = next.to_string();
        if let Some(expression) = self.bound_expression.as_mut() {
            expression.rename_column_references(&current_key, next);
        }
        true
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct DefaultSequenceOwnership(bool);

impl DefaultSequenceOwnership {
    #[must_use]
    pub const fn shared() -> Self { Self(false) }
    #[must_use]
    pub const fn owned() -> Self { Self(true) }
    #[must_use]
    pub const fn is_owned(self) -> bool { self.0 }
}

impl Default for DefaultSequenceOwnership {
    fn default() -> Self { Self::shared() }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotNullOwnership {
    #[default]
    None,
    Explicit,
    PrimaryKey,
    ExplicitAndPrimaryKey,
}

impl NotNullOwnership {
    #[must_use]
    pub const fn with_explicit(self) -> Self {
        match self {
            Self::None | Self::Explicit => Self::Explicit,
            Self::PrimaryKey | Self::ExplicitAndPrimaryKey => Self::ExplicitAndPrimaryKey,
        }
    }

    #[must_use]
    pub const fn with_primary_key(self) -> Self {
        match self {
            Self::None | Self::PrimaryKey => Self::PrimaryKey,
            Self::Explicit | Self::ExplicitAndPrimaryKey => Self::ExplicitAndPrimaryKey,
        }
    }

    #[must_use]
    pub const fn without_primary_key(self) -> Self {
        match self {
            Self::PrimaryKey => Self::None,
            Self::ExplicitAndPrimaryKey => Self::Explicit,
            other => other,
        }
    }

    #[must_use]
    pub const fn is_explicit(self) -> bool {
        matches!(self, Self::Explicit | Self::ExplicitAndPrimaryKey)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = ""))]
pub struct FieldConstraint<A = ()> {
    /// Durable expression contract: zero is legacy; one admits selected null-safe predicates.
    #[serde(default, skip_serializing_if = "crate::definition_guard::is_legacy")]
    pub definition_semantics_version: u32,
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
    pub constraint_index_name: Option<String>,
    #[serde(default)]
    pub constraint_index_include_fields: Vec<String>,
    #[serde(default)]
    pub constraint_index_options: std::collections::BTreeMap<String, String>,
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
    pub not_null_ownership: NotNullOwnership,
    #[serde(default)]
    pub unique: bool,
    #[serde(default)]
    pub unique_null_policy: cassie_core::catalog::declarations::UniqueNullPolicy,
    #[serde(default)]
    pub primary_key: bool,
    #[serde(default)]
    pub default_value: Option<serde_json::Value>,
    #[serde(default)]
    pub default_expression: Option<String>,
    #[serde(default)]
    pub default_sequence: Option<String>,
    #[serde(default)]
    pub default_sequence_owned: DefaultSequenceOwnership,
    #[serde(default)]
    pub check: Option<ConstraintCheck<A>>,
    #[serde(default)]
    pub references_table: Option<String>,
    #[serde(default)]
    pub references_field: Option<String>,
    #[serde(default)]
    pub identity: Option<cassie_core::catalog::declarations::IdentityGeneration>,
    #[serde(default)]
    pub identity_options: Option<cassie_core::catalog::declarations::SequenceOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<cassie_core::catalog::column_storage::ColumnStorage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compression: Option<cassie_core::catalog::column_storage::ColumnCompression>,
}

impl<A: std::fmt::Debug> PartialEq for FieldConstraint<A> {
    fn eq(&self, other: &Self) -> bool {
        self.definition_semantics_version == other.definition_semantics_version
            && self.field == other.field
            && self.primary_key_name == other.primary_key_name
            && self.primary_key_ordinal == other.primary_key_ordinal
            && self.unique_name == other.unique_name
            && self.unique_ordinal == other.unique_ordinal
            && self.constraint_index_name == other.constraint_index_name
            && self.constraint_index_include_fields == other.constraint_index_include_fields
            && self.constraint_index_options == other.constraint_index_options
            && self.check_name == other.check_name
            && self.foreign_key_name == other.foreign_key_name
            && self.foreign_key_ordinal == other.foreign_key_ordinal
            && self.foreign_key_on_delete == other.foreign_key_on_delete
            && self.foreign_key_on_update == other.foreign_key_on_update
            && self.not_null == other.not_null
            && self.not_null_ownership == other.not_null_ownership
            && self.unique == other.unique
            && self.unique_null_policy == other.unique_null_policy
            && self.primary_key == other.primary_key
            && self.default_value == other.default_value
            && self.default_expression == other.default_expression
            && self.default_sequence == other.default_sequence
            && self.default_sequence_owned == other.default_sequence_owned
            && self.check == other.check
            && self.references_table == other.references_table
            && self.references_field == other.references_field
            && self.identity == other.identity
            && self.identity_options == other.identity_options
            && self.storage == other.storage
            && self.compression == other.compression
    }
}

impl<A> FieldConstraint<A> {
    pub fn new(field: impl Into<String>) -> Self {
        Self {
            definition_semantics_version: 0,
            field: field.into(),
            primary_key_name: None,
            primary_key_ordinal: None,
            unique_name: None,
            unique_ordinal: None,
            constraint_index_name: None,
            constraint_index_include_fields: Vec::new(),
            constraint_index_options: std::collections::BTreeMap::new(),
            check_name: None,
            foreign_key_name: None,
            foreign_key_ordinal: None,
            foreign_key_on_delete: None,
            foreign_key_on_update: None,
            not_null: false,
            not_null_ownership: NotNullOwnership::None,
            unique: false,
            unique_null_policy: cassie_core::catalog::declarations::UniqueNullPolicy::Distinct,
            primary_key: false,
            default_value: None,
            default_expression: None,
            default_sequence: None,
            default_sequence_owned: DefaultSequenceOwnership::shared(),
            check: None,
            references_table: None,
            references_field: None,
            identity: None,
            identity_options: None,
            storage: None,
            compression: None,
        }
    }

    pub fn use_declared_field_spelling(&mut self, declared: &str) {
        let current = crate::column_identifier::ColumnIdentifierPath::reference_field_key(&self.field);
        if self.field != declared
            && current != crate::column_identifier::ColumnIdentifierPath::stored_field_key(declared)
        { return; }
        if let Some(check) = self.check.as_mut().filter(|check| {
            check.field == declared
                || crate::column_identifier::ColumnIdentifierPath::reference_field_key(&check.field)
                    == crate::column_identifier::ColumnIdentifierPath::stored_field_key(declared)
        }) { check.field = declared.to_string(); }
        self.field = declared.to_string();
    }

    #[must_use]
    pub fn unique_constraint_name(&self, collection: &str) -> String {
        let (declared, kind) = if self.primary_key {
            (self.primary_key_name.as_ref(), "PRIMARY KEY")
        } else {
            (self.unique_name.as_ref(), "UNIQUE")
        };
        declared
            .cloned()
            .unwrap_or_else(|| generated_constraint_name(collection, &self.field, kind))
    }

    #[must_use]
    pub fn foreign_key_constraint_name(&self, collection: &str) -> String {
        self.foreign_key_name
            .clone()
            .unwrap_or_else(|| generated_constraint_name(collection, &self.field, "FOREIGN KEY"))
    }

    pub fn clear_foreign_key(&mut self) {
        self.references_table = None;
        self.references_field = None;
        self.foreign_key_name = None;
        self.foreign_key_ordinal = None;
        self.foreign_key_on_delete = None;
        self.foreign_key_on_update = None;
    }
}

#[must_use]
pub fn generated_constraint_name(collection: &str, field: &str, kind: &str) -> String {
    format!(
        "{}_{}_{}",
        cassie_core::catalog::scope::local_name(collection),
        field,
        kind.to_ascii_lowercase().replace(' ', "_")
    )
}

/// Returns a diagnostic when one column would join two multi-column UNIQUE constraints.
#[must_use]
pub fn conflicting_unique_membership<A: Clone>(
    collection: &str,
    existing: &[FieldConstraint<A>],
    additions: &[FieldConstraint<A>],
) -> Option<String> {
    let mut combined = existing.to_vec();
    combined.extend(additions.iter().cloned());
    for addition in additions.iter().filter(|addition| addition.unique) {
        let Some(current) = existing.iter().find(|entry| {
            entry.unique
                && crate::column_identifier::ColumnIdentifierPath::stored_field_key(&entry.field)
                    == crate::column_identifier::ColumnIdentifierPath::stored_field_key(&addition.field)
        }) else {
            continue;
        };
        let current_name = unique_name(collection, current);
        let added_name = unique_name(collection, addition);
        if current_name.eq_ignore_ascii_case(&added_name) {
            continue;
        }
        let spans_columns = |name: &str| {
            combined.iter().filter(|entry| {
                entry.unique && unique_name(collection, entry).eq_ignore_ascii_case(name)
            }).nth(1).is_some()
        };
        if spans_columns(&current_name) || spans_columns(&added_name) {
            return Some(format!(
                "column '{}' cannot be in both UNIQUE constraint '{current_name}' and '{added_name}': a column may belong to only one multi-column UNIQUE constraint",
                addition.field
            ));
        }
    }
    None
}

fn unique_name<A>(collection: &str, constraint: &FieldConstraint<A>) -> String {
    constraint.unique_name.clone().unwrap_or_else(|| {
        generated_constraint_name(collection, &constraint.field, "UNIQUE")
    })
}

pub fn merge_constraint_set<A>(
    existing: &mut Vec<FieldConstraint<A>>,
    additions: impl IntoIterator<Item = FieldConstraint<A>>,
) {
    for addition in additions {
        if !is_constraint_populated(&addition) {
            continue;
        }
        if let Some(current) = existing.iter_mut().find(|entry| {
            crate::column_identifier::ColumnIdentifierPath::stored_field_key(&entry.field)
                == crate::column_identifier::ColumnIdentifierPath::stored_field_key(&addition.field)
        }) {
            merge_constraint(current, addition);
        } else {
            existing.push(addition);
        }
    }
}

fn is_constraint_populated<A>(constraint: &FieldConstraint<A>) -> bool {
    constraint.primary_key
        || constraint.unique
        || constraint.not_null
        || constraint.default_value.is_some()
        || constraint.default_expression.is_some()
        || constraint.default_sequence.is_some()
        || constraint.check.is_some()
        || constraint.references_table.is_some()
        || constraint.storage.is_some()
        || constraint.compression.is_some()
}

fn merge_constraint<A>(existing: &mut FieldConstraint<A>, next: FieldConstraint<A>) {
    existing.definition_semantics_version = existing
        .definition_semantics_version
        .max(next.definition_semantics_version);
    existing.not_null |= next.not_null;
    if next.not_null_ownership.is_explicit() {
        existing.not_null_ownership = existing.not_null_ownership.with_explicit();
    }
    if matches!(
        next.not_null_ownership,
        NotNullOwnership::PrimaryKey | NotNullOwnership::ExplicitAndPrimaryKey
    ) {
        existing.not_null_ownership = existing.not_null_ownership.with_primary_key();
    }
    existing.unique |= next.unique;
    if next.unique_null_policy == cassie_core::catalog::declarations::UniqueNullPolicy::NotDistinct {
        existing.unique_null_policy = cassie_core::catalog::declarations::UniqueNullPolicy::NotDistinct;
    }
    existing.primary_key |= next.primary_key;
    if next.primary_key_name.is_some() { existing.primary_key_name = next.primary_key_name; }
    if next.primary_key_ordinal.is_some() { existing.primary_key_ordinal = next.primary_key_ordinal; }
    if next.unique_name.is_some() { existing.unique_name = next.unique_name; }
    if next.unique_ordinal.is_some() { existing.unique_ordinal = next.unique_ordinal; }
    if next.default_value.is_some() { existing.default_value = next.default_value; }
    if next.default_expression.is_some() { existing.default_expression = next.default_expression; }
    if next.default_sequence.is_some() { existing.default_sequence = next.default_sequence; }
    if next.default_sequence_owned.is_owned() { existing.default_sequence_owned = next.default_sequence_owned; }
    if next.check.is_some() { existing.check = next.check; }
    if next.check_name.is_some() { existing.check_name = next.check_name; }
    if next.references_table.is_some() {
        existing.references_table = next.references_table;
        existing.references_field = next.references_field;
    }
    if next.foreign_key_name.is_some() { existing.foreign_key_name = next.foreign_key_name; }
    if next.foreign_key_ordinal.is_some() { existing.foreign_key_ordinal = next.foreign_key_ordinal; }
    if next.foreign_key_on_delete.is_some() { existing.foreign_key_on_delete = next.foreign_key_on_delete; }
    if next.foreign_key_on_update.is_some() { existing.foreign_key_on_update = next.foreign_key_on_update; }
    if next.storage.is_some() { existing.storage = next.storage; }
    if next.compression.is_some() { existing.compression = next.compression; }
}
