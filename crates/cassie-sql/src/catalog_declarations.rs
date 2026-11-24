//! SQL-aware persisted index declaration and portable metadata operations.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use cassie_core::catalog::projection::{
    MaterializedProjectionState, ProjectionFreshness, ProjectionHashMeta, ProjectionIntegrityReportMeta,
    ProjectionKind, ProjectionRebuildState, ProjectionRebuildVerificationMeta, ProjectionSwapMeta,
    ProjectionVerificationState, ProjectionVersionMeta, ProjectionVersionState,
};

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

    /// Whether dropping `field` invalidates this index through its stored
    /// columns, scalar expression or predicate, partition key, or vector source.
    #[must_use]
    pub fn references_field(&self, field: &str) -> bool {
        self.references_field_checked(field).unwrap_or(true)
    }

    /// Checks stored expression definitions and reports malformed metadata so
    /// DROP COLUMN can abort before publishing any schema changes.
    pub fn references_field_checked(&self, field: &str) -> Result<bool, String> {
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
                });
        if column_references_field || self.kind != IndexKind::Scalar {
            return Ok(column_references_field);
        }

        let field_key = crate::column_identifier::ColumnIdentifierPath::stored_field_key(field);
        for raw in self.expressions.iter().chain(self.predicate.iter()) {
            let expression = serde_json::from_str::<crate::ast::Expr>(raw).map_err(|error| {
                format!("invalid expression metadata for index '{}': {error}", self.name)
            })?;
            if expression.any_descendant_or_self(&mut |node| {
                matches!(
                    node,
                    crate::ast::Expr::Column(column)
                        if crate::column_identifier::ColumnIdentifierPath::reference_field_key(column) == field_key
                )
            }) {
                return Ok(true);
            }
        }
        Ok(false)
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



/// Option key marking a materialized projection created by SQL materialized-view DDL.
pub const SQL_MATERIALIZED_VIEW_OPTION: &str = "cassie.sql_materialized_view";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterializedProjectionMeta {
    /// Durable expression contract: zero is legacy; one admits selected null-safe predicates.
    #[serde(
        default,
        skip_serializing_if = "crate::definition_guard::is_legacy"
    )]
    pub definition_semantics_version: u32,
    pub name: String,
    pub query: String,
    #[serde(default)]
    pub options: std::collections::BTreeMap<String, String>,
    pub output_collection: String,
    pub source_collections: Vec<String>,
    pub schema_epoch: u64,
    pub output_schema: cassie_core::types::Schema,
    pub state: MaterializedProjectionState,
    pub definition_fingerprint: u64,
    pub refresh_cursor: Option<String>,
    pub last_built_ms: Option<u64>,
}



#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionMeta {
    #[serde(default)]
    pub projection_id: String,
    pub collection: String,
    #[serde(default)]
    pub kind: ProjectionKind,
    pub schema_version: u32,
    pub offset: u64,
    pub lag: u64,
    pub rebuild_state: ProjectionRebuildState,
    #[serde(default)]
    pub source_identity: Option<String>,
    #[serde(default)]
    pub source_generations: BTreeMap<String, u64>,
    #[serde(default)]
    pub source_checkpoint: Option<String>,
    #[serde(default)]
    pub source_position: Option<u64>,
    #[serde(default)]
    pub last_applied_event_id: Option<String>,
    #[serde(default)]
    pub replay_batch_id: Option<String>,
    #[serde(default)]
    pub applied_event_count: u64,
    #[serde(default)]
    pub skipped_duplicate_count: u64,
    #[serde(default)]
    pub freshness: ProjectionFreshness,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub materialized: Option<MaterializedProjectionMeta>,
    #[serde(default)]
    pub versions: Vec<ProjectionVersionMeta>,
    #[serde(default)]
    pub active_version: Option<String>,
    #[serde(default)]
    pub swap: ProjectionSwapMeta,
    #[serde(default)]
    pub hashes: ProjectionHashMeta,
    #[serde(default)]
    pub verification: ProjectionRebuildVerificationMeta,
    #[serde(default)]
    pub integrity: ProjectionIntegrityReportMeta,
    /// Highest version ordinal ever allocated, so dropped version ids are never
    /// reused. Metadata written before this field existed reads as zero.
    #[serde(default)]
    pub version_high_water: u64,
}

impl ProjectionMeta {
    pub fn new(collection: impl Into<String>, schema_version: u32) -> Self {
        let collection = collection.into();
        Self {
            projection_id: collection.clone(),
            collection,
            kind: ProjectionKind::Collection,
            schema_version,
            offset: 0,
            lag: 0,
            rebuild_state: ProjectionRebuildState::Idle,
            source_identity: None,
            source_generations: BTreeMap::new(),
            source_checkpoint: None,
            source_position: None,
            last_applied_event_id: None,
            replay_batch_id: None,
            applied_event_count: 0,
            skipped_duplicate_count: 0,
            freshness: ProjectionFreshness::Unknown,
            last_error: None,
            materialized: None,
            versions: Vec::new(),
            active_version: None,
            swap: ProjectionSwapMeta::default(),
            hashes: ProjectionHashMeta::default(),
            verification: ProjectionRebuildVerificationMeta::default(),
            integrity: ProjectionIntegrityReportMeta::default(),
            version_high_water: 0,
        }
    }

    #[must_use]
    pub fn materialized(spec: MaterializedProjectionSpec) -> Self {
        let name = spec.name;
        let version_id = "v1".to_string();
        let output_collection = materialized_output_collection(&name, &version_id);
        Self {
            projection_id: name.clone(),
            collection: name.clone(),
            kind: ProjectionKind::Materialized,
            schema_version: 1,
            offset: 0,
            lag: 0,
            rebuild_state: ProjectionRebuildState::Rebuilding,
            source_identity: spec.source_collections.first().cloned(),
            source_generations: BTreeMap::new(),
            source_checkpoint: None,
            source_position: None,
            last_applied_event_id: None,
            replay_batch_id: None,
            applied_event_count: 0,
            skipped_duplicate_count: 0,
            freshness: ProjectionFreshness::Rebuilding,
            last_error: None,
            materialized: Some(MaterializedProjectionMeta {
                definition_semantics_version: 1,
                name: name.clone(),
                query: spec.query,
                options: spec.options,
                output_collection: output_collection.clone(),
                source_collections: spec.source_collections,
                schema_epoch: spec.schema_epoch,
                output_schema: spec.output_schema,
                state: MaterializedProjectionState::Building,
                definition_fingerprint: spec.definition_fingerprint,
                refresh_cursor: None,
                last_built_ms: None,
            }),
            versions: vec![ProjectionVersionMeta {
                version_id: version_id.clone(),
                output_collection,
                definition_fingerprint: spec.definition_fingerprint,
                source_schema_epoch: spec.schema_epoch,
                state: ProjectionVersionState::Building,
                created_ms: spec.created_ms,
                activated_ms: None,
                retired_ms: None,
                last_error: None,
                verification: ProjectionRebuildVerificationMeta::default(),
            }],
            active_version: None,
            swap: ProjectionSwapMeta::default(),
            hashes: ProjectionHashMeta::default(),
            verification: ProjectionRebuildVerificationMeta::default(),
            integrity: ProjectionIntegrityReportMeta::default(),
            version_high_water: 1,
        }
    }

    /// Allocates the next `v<N>` version id past both the persisted high-water
    /// ordinal and every live version, and records it as the new high water.
    pub fn allocate_version_id(&mut self) -> String {
        let live = self
            .versions
            .iter()
            .map(|version| version.version_id.as_str())
            .chain(self.active_version.as_deref())
            .filter_map(|version_id| version_id.strip_prefix('v')?.parse::<u64>().ok())
            .max()
            .unwrap_or(0);
        self.version_high_water = self.version_high_water.max(live) + 1;
        format!("v{}", self.version_high_water)
    }

    #[must_use]
    pub fn projection_id(&self) -> &str {
        if self.projection_id.is_empty() {
            &self.collection
        } else {
            &self.projection_id
        }
    }

    #[must_use]
    pub fn active_output_collection(&self) -> Option<&str> {
        let active = self.active_version.as_ref()?;
        self.versions
            .iter()
            .find(|version| &version.version_id == active)
            .map(|version| version.output_collection.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct MaterializedProjectionSpec {
    pub name: String,
    pub query: String,
    pub source_collections: Vec<String>,
    pub output_schema: cassie_core::types::Schema,
    pub options: std::collections::BTreeMap<String, String>,
    pub schema_epoch: u64,
    pub definition_fingerprint: u64,
    pub created_ms: u64,
}

#[must_use]
pub fn materialized_output_collection(name: &str, version_id: &str) -> String {
    cassie_core::catalog::scope::derive_scoped_name(name, |local| {
        format!("__cassie_projection_{local}_{version_id}")
    })
}


#[cfg(test)]
mod tests {
    use super::{IndexKind, IndexMeta, MaterializedProjectionSpec, ProjectionMeta};
    use cassie_core::types::Schema;
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

    #[test]
    fn should_preserve_projection_metadata_and_materialized_definition_encoding() {
        // Arrange
        let projection = ProjectionMeta::new("public.events", 3);
        let materialized = ProjectionMeta::materialized(MaterializedProjectionSpec {
            name: "public.events_view".to_owned(),
            query: "SELECT id FROM public.events".to_owned(),
            source_collections: vec!["public.events".to_owned()],
            output_schema: Schema { fields: Vec::new() },
            options: BTreeMap::new(),
            schema_epoch: 8,
            definition_fingerprint: 9,
            created_ms: 10,
        });

        // Act
        let projection_bytes = serde_json::to_value(&projection).expect("serialize projection");
        let materialized_bytes =
            serde_json::to_value(&materialized).expect("serialize materialized projection");

        // Assert
        assert_eq!(projection_bytes["projection_id"], "public.events");
        assert_eq!(projection_bytes["version_high_water"], 0);
        assert_eq!(
            materialized_bytes["materialized"]["definition_semantics_version"],
            1
        );
        assert_eq!(materialized_bytes["materialized"]["query"], "SELECT id FROM public.events");
        assert_eq!(materialized_bytes["versions"][0]["version_id"], "v1");
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
