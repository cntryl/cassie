use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;

use crate::catalog::{
    local_name, name_matches, parse_name, CollectionCardinalityStats, CollectionMeta,
    CollectionSchema, CollectionStorageMode, DatabaseMeta, FieldConstraint, FieldMeta,
    FunctionMeta, GraphMeta, IndexKind, IndexMeta, NamespaceMeta, OperationalAssignmentMeta,
    ProcedureMeta, ProjectionComparisonReportMeta, ProjectionConsistencyReportMeta, ProjectionMeta,
    ProjectionRepairReportMeta, RetentionPolicyMeta, RoleMeta, RollupMeta, SequenceMeta, ViewMeta,
};
use crate::embeddings::VectorIndexRecord;
use crate::types::{DataType, Schema};

#[derive(Debug, Clone)]
pub struct Catalog {
    pub(super) databases: Arc<RwLock<HashMap<String, DatabaseMeta>>>,
    pub(super) collections: Arc<RwLock<HashMap<String, CollectionMeta>>>,
    pub(super) namespaces: Arc<RwLock<HashMap<String, NamespaceMeta>>>,
    pub(super) schemas: Arc<RwLock<HashMap<String, CollectionSchema>>>,
    pub(super) projections: Arc<RwLock<HashMap<String, ProjectionMeta>>>,
    pub(super) constraints: Arc<RwLock<HashMap<String, Vec<FieldConstraint>>>>,
    pub(super) indexes: Arc<RwLock<HashMap<String, IndexMeta>>>,
    pub(super) graphs: Arc<RwLock<HashMap<String, GraphMeta>>>,
    pub(super) functions: Arc<RwLock<HashMap<String, FunctionMeta>>>,
    pub(super) procedures: Arc<RwLock<HashMap<String, ProcedureMeta>>>,
    pub(super) views: Arc<RwLock<HashMap<String, ViewMeta>>>,
    pub(super) roles: Arc<RwLock<HashMap<String, RoleMeta>>>,
    pub(super) sequences: Arc<RwLock<HashMap<String, SequenceMeta>>>,
    pub(super) rollups: Arc<RwLock<HashMap<String, RollupMeta>>>,
    pub(super) retention_policies: Arc<RwLock<HashMap<String, RetentionPolicyMeta>>>,
    pub(super) vector_indexes: Arc<RwLock<HashMap<String, VectorIndexRecord>>>,
    pub(super) cardinality: Arc<RwLock<HashMap<String, CollectionCardinalityStats>>>,
    pub(super) projection_comparison_reports:
        Arc<RwLock<HashMap<String, ProjectionComparisonReportMeta>>>,
    pub(super) projection_consistency_reports:
        Arc<RwLock<HashMap<String, ProjectionConsistencyReportMeta>>>,
    pub(super) projection_repair_reports: Arc<RwLock<HashMap<String, ProjectionRepairReportMeta>>>,
    pub(super) operational_assignments: Arc<RwLock<HashMap<String, OperationalAssignmentMeta>>>,
    version: Arc<AtomicU64>,
}

#[path = "metadata_databases.rs"]
mod metadata_databases;
#[path = "metadata_domains.rs"]
mod metadata_domains;

impl Catalog {
    #[must_use]
    pub fn new() -> Self {
        Self {
            databases: Arc::new(RwLock::new(HashMap::new())),
            collections: Arc::new(RwLock::new(HashMap::new())),
            namespaces: Arc::new(RwLock::new(HashMap::new())),
            schemas: Arc::new(RwLock::new(HashMap::new())),
            projections: Arc::new(RwLock::new(HashMap::new())),
            constraints: Arc::new(RwLock::new(HashMap::new())),
            indexes: Arc::new(RwLock::new(HashMap::new())),
            graphs: Arc::new(RwLock::new(HashMap::new())),
            functions: Arc::new(RwLock::new(HashMap::new())),
            procedures: Arc::new(RwLock::new(HashMap::new())),
            views: Arc::new(RwLock::new(HashMap::new())),
            roles: Arc::new(RwLock::new(HashMap::new())),
            sequences: Self::sequence_store(),
            rollups: Arc::new(RwLock::new(HashMap::new())),
            retention_policies: Arc::new(RwLock::new(HashMap::new())),
            vector_indexes: Arc::new(RwLock::new(HashMap::new())),
            cardinality: Arc::new(RwLock::new(HashMap::new())),
            projection_comparison_reports: Arc::new(RwLock::new(HashMap::new())),
            projection_consistency_reports: Arc::new(RwLock::new(HashMap::new())),
            projection_repair_reports: Arc::new(RwLock::new(HashMap::new())),
            operational_assignments: Arc::new(RwLock::new(HashMap::new())),
            version: Arc::new(AtomicU64::new(0)),
        }
    }

    #[must_use]
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
    }

    pub fn register_collection(&self, name: &str, schema: Vec<(String, DataType)>) {
        self.register_collection_with_constraints(name, schema, Vec::new());
    }

    pub fn register_collection_with_constraints(
        &self,
        name: &str,
        schema: Vec<(String, DataType)>,
        constraints: Vec<FieldConstraint>,
    ) {
        self.register_collection_meta_with_constraints(
            CollectionMeta::new(name, None),
            schema,
            constraints,
        );
    }

    pub fn register_collection_meta_with_constraints(
        &self,
        metadata: CollectionMeta,
        schema: Vec<(String, DataType)>,
        constraints: Vec<FieldConstraint>,
    ) {
        let mut collections = self.collections.write();
        let name = metadata.name.clone();
        collections.insert(name.clone(), metadata);

        let mut schemas = self.schemas.write();
        let fields = schema
            .into_iter()
            .map(|(name, data_type)| crate::catalog::FieldMeta {
                name,
                data_type,
                is_indexed: true,
                boost: Some(1.0),
            })
            .collect();
        schemas.insert(
            name.clone(),
            CollectionSchema {
                collection: name.clone(),
                fields,
            },
        );

        let normalized = constraints
            .into_iter()
            .filter(Self::is_constraint_populated)
            .collect::<Vec<_>>();
        self.constraints.write().insert(name.clone(), normalized);
        self.register_projection_metadata(ProjectionMeta::new(&name, 1));
        self.cardinality.write().insert(
            name.clone(),
            CollectionCardinalityStats {
                hydrated: false,
                ..CollectionCardinalityStats::default()
            },
        );
        self.bump_version();
    }

    #[must_use]
    pub fn get_collection(&self, name: &str) -> Option<CollectionMeta> {
        let collections = self.collections.read();
        collections.get(name).cloned().or_else(|| {
            collections
                .iter()
                .find(|(stored, _)| name_matches(stored, name))
                .map(|(_, metadata)| metadata.clone())
        })
    }

    #[must_use]
    pub fn collection_storage_mode(&self, name: &str) -> Option<CollectionStorageMode> {
        let base = self.get_collection(name)?.storage_mode;
        if matches!(
            base,
            CollectionStorageMode::ColumnStore | CollectionStorageMode::ColumnIndexed
        ) {
            return Some(base);
        }
        let has_column_index = self
            .indexes
            .read()
            .values()
            .any(|index| index.collection == name && index.kind == IndexKind::Column);
        Some(if has_column_index {
            CollectionStorageMode::ColumnIndexed
        } else {
            CollectionStorageMode::RowStore
        })
    }

    #[must_use]
    pub fn list_collections(&self) -> Vec<CollectionMeta> {
        let mut out = self.list_collections_canonical();
        for collection in &mut out {
            collection.name = local_name(&collection.name);
        }
        out
    }

    #[must_use]
    pub(crate) fn list_collections_canonical(&self) -> Vec<CollectionMeta> {
        let collections = self.collections.read();
        let mut out = collections.values().cloned().collect::<Vec<_>>();
        out.sort_by_key(|entry| entry.name.to_ascii_lowercase());
        out
    }

    #[must_use]
    pub fn namespace_exists(&self, namespace: &str) -> bool {
        let namespaces = self.namespaces.read();
        namespaces.contains_key(namespace)
            || namespaces
                .keys()
                .any(|stored| name_matches(stored, namespace))
    }

    pub fn clear(&self) {
        self.databases.write().clear();
        self.collections.write().clear();
        self.namespaces.write().clear();
        self.schemas.write().clear();
        self.projections.write().clear();
        self.constraints.write().clear();
        self.functions.write().clear();
        self.procedures.write().clear();
        self.views.write().clear();
        self.roles.write().clear();
        self.sequences.write().clear();
        self.rollups.write().clear();
        self.retention_policies.write().clear();
        self.indexes.write().clear();
        self.graphs.write().clear();
        self.vector_indexes.write().clear();
        self.cardinality.write().clear();
        self.projection_comparison_reports.write().clear();
        self.projection_consistency_reports.write().clear();
        self.projection_repair_reports.write().clear();
        self.operational_assignments.write().clear();
        self.bump_version();
    }

    pub fn unregister_collection(&self, collection: &str) {
        self.collections.write().remove(collection);
        self.schemas.write().remove(collection);
        self.projections.write().remove(collection);
        self.constraints.write().remove(collection);
        self.indexes
            .write()
            .retain(|_, index| index.collection != collection);
        self.vector_indexes
            .write()
            .retain(|_, record| record.collection != collection);
        self.cardinality.write().remove(collection);
        self.rollups
            .write()
            .retain(|_, rollup| rollup.source_collection != collection);
        self.retention_policies
            .write()
            .retain(|_, policy| policy.collection != collection);
        self.projection_comparison_reports
            .write()
            .retain(|_, report| report.target != collection);
        self.projection_consistency_reports
            .write()
            .retain(|_, report| report.projection_id != collection);
        self.projection_repair_reports
            .write()
            .retain(|_, report| report.projection_name != collection);
        self.bump_version();
    }

    #[must_use]
    pub fn get_constraints(&self, collection: &str) -> Vec<FieldConstraint> {
        let constraints = self.constraints.read();
        constraints
            .get(collection)
            .cloned()
            .or_else(|| {
                constraints
                    .iter()
                    .find(|(stored, _)| name_matches(stored, collection))
                    .map(|(_, value)| value.clone())
            })
            .unwrap_or_default()
    }

    #[must_use]
    pub fn get_constraint(&self, collection: &str, field: &str) -> Option<FieldConstraint> {
        self.constraints
            .read()
            .get(collection)
            .and_then(|constraints| {
                constraints
                    .iter()
                    .find(|constraint| constraint.field.eq_ignore_ascii_case(field))
                    .cloned()
            })
    }

    pub fn register_constraints(&self, collection: &str, constraints: Vec<FieldConstraint>) {
        let normalized = constraints
            .into_iter()
            .filter(Self::is_constraint_populated)
            .collect::<Vec<_>>();
        self.constraints
            .write()
            .insert(collection.to_string(), normalized);
        self.bump_version();
    }

    pub fn replace_collection_constraint_set(
        &self,
        collection: &str,
        constraints: Vec<FieldConstraint>,
    ) {
        self.register_constraints(collection, constraints);
    }

    pub fn replace_constraints_for_field(
        &self,
        collection: &str,
        field: &str,
        constraint: Option<FieldConstraint>,
    ) {
        let mut constraints = self.constraints.write();
        let Some(entries) = constraints.get_mut(collection) else {
            return;
        };

        let position = entries.iter().position(|entry| entry.field == field);
        match (position, constraint) {
            (Some(position), Some(constraint)) => {
                entries[position] = constraint;
            }
            (Some(position), None) => {
                entries.remove(position);
            }
            (None, Some(constraint)) => entries.push(constraint),
            (None, None) => {}
        }
        self.bump_version();
    }

    pub fn register_index(&self, metadata: IndexMeta) {
        let mut indexes = self.indexes.write();
        indexes.insert(
            Self::index_key(&metadata.collection, &metadata.name),
            metadata,
        );
        self.bump_version();
    }

    pub fn unregister_index(&self, collection: &str, name: &str) {
        let mut indexes = self.indexes.write();
        let key = Self::index_key(collection, name);
        if indexes.remove(&key).is_none() {
            let matching_key = indexes
                .iter()
                .find(|(_, index)| {
                    name_matches(&index.collection, collection) && name_matches(&index.name, name)
                })
                .map(|(stored_key, _)| stored_key.clone());
            if let Some(stored_key) = matching_key {
                indexes.remove(&stored_key);
            }
        }
        self.bump_version();
    }

    #[must_use]
    pub fn get_index(&self, collection: &str, name: &str) -> Option<IndexMeta> {
        let indexes = self.indexes.read();
        let metadata = indexes
            .get(&Self::index_key(collection, name))
            .cloned()
            .or_else(|| {
                indexes
                    .values()
                    .find(|index| {
                        name_matches(&index.collection, collection)
                            && name_matches(&index.name, name)
                    })
                    .cloned()
            })?;
        if matches!(
            parse_name(collection),
            Ok(crate::catalog::ParsedName::Unqualified(_))
        ) {
            let mut display = metadata;
            display.collection = local_name(&display.collection);
            Some(display)
        } else {
            Some(metadata)
        }
    }

    #[must_use]
    pub fn list_indexes(&self, collection: &str) -> Vec<IndexMeta> {
        let indexes = self.indexes.read();
        let mut out = indexes
            .values()
            .filter(|index| name_matches(&index.collection, collection))
            .cloned()
            .collect::<Vec<_>>();
        out.sort_by_key(|index| index.name.to_ascii_lowercase());
        out
    }

    #[must_use]
    pub fn all_indexes_snapshot(&self) -> Vec<IndexMeta> {
        let mut out = self.indexes.read().values().cloned().collect::<Vec<_>>();
        out.sort_by(|left, right| {
            left.collection
                .to_ascii_lowercase()
                .cmp(&right.collection.to_ascii_lowercase())
                .then_with(|| {
                    left.name
                        .to_ascii_lowercase()
                        .cmp(&right.name.to_ascii_lowercase())
                })
        });
        out
    }

    #[must_use]
    pub fn get_schema(&self, collection: &str) -> Option<CollectionSchema> {
        let schemas = self.schemas.read();
        if let Some(schema) = schemas.get(collection).cloned() {
            return Some(schema);
        }
        if let Some(schema) = schemas
            .iter()
            .find(|(stored, _)| name_matches(stored, collection))
            .map(|(_, schema)| schema.clone())
        {
            return Some(schema);
        }
        drop(schemas);

        let views = self.views.read();
        views
            .get(collection)
            .or_else(|| {
                views
                    .iter()
                    .find(|(stored, _)| name_matches(stored, collection))
                    .map(|(_, view)| view)
            })
            .map(|view| view_schema_to_collection_schema(&view.name, &view.schema))
    }

    #[must_use]
    pub fn field_type(&self, collection: &str, field: &str) -> Option<DataType> {
        self.get_schema(collection).and_then(|schema| {
            schema
                .fields
                .into_iter()
                .find(|entry| entry.name.eq_ignore_ascii_case(field))
                .map(|entry| entry.data_type)
        })
    }

    pub fn add_collection_field(&self, collection: &str, name: String, data_type: DataType) {
        let mut schemas = self.schemas.write();
        let Some(schema) = schemas.get_mut(collection) else {
            return;
        };

        if schema.fields.iter().any(|field| field.name == name) {
            return;
        }

        schema.fields.push(crate::catalog::FieldMeta {
            name,
            data_type,
            is_indexed: true,
            boost: Some(1.0),
        });
        self.bump_version();
    }

    pub fn remove_collection_field(&self, collection: &str, name: &str) {
        let mut schemas = self.schemas.write();
        let Some(schema) = schemas.get_mut(collection) else {
            return;
        };

        schema.fields.retain(|field| field.name != name);
        self.indexes.write().retain(|_, index| {
            index.collection != collection
                || index.kind != IndexKind::Column
                || !index
                    .normalized_fields()
                    .iter()
                    .any(|field| field.eq_ignore_ascii_case(name))
        });
        self.retention_policies.write().retain(|_, policy| {
            policy.collection != collection || !policy.timestamp_field.eq_ignore_ascii_case(name)
        });
        self.bump_version();
    }

    pub fn rename_collection(&self, current_name: &str, next_name: &str) {
        let mut collections = self.collections.write();
        let metadata = collections.remove(current_name);
        if let Some(mut metadata) = metadata {
            metadata.name = next_name.to_string();
            collections.insert(next_name.to_string(), metadata);
        }

        let mut schemas = self.schemas.write();
        if let Some(schema) = schemas.remove(current_name) {
            schemas.insert(
                next_name.to_string(),
                CollectionSchema {
                    collection: next_name.to_string(),
                    fields: schema.fields,
                },
            );
        }
        let mut projections = self.projections.write();
        if let Some(mut projection) = projections.remove(current_name) {
            projection.collection = next_name.to_string();
            if projection.projection_id == current_name {
                projection.projection_id = next_name.to_string();
            }
            projections.insert(next_name.to_string(), projection);
        }

        let normalized_constraints = {
            let mut constraints = self.constraints.write();
            constraints.remove(current_name).unwrap_or_default()
        };
        if !normalized_constraints.is_empty() {
            self.constraints
                .write()
                .insert(next_name.to_string(), normalized_constraints);
        }

        let mut indexes = self.indexes.write();
        let existing_indexes = indexes
            .iter()
            .filter(|(_, index)| index.collection == current_name)
            .map(|(key, index)| (key.clone(), index.clone()))
            .collect::<Vec<_>>();

        for (key, mut index) in existing_indexes {
            indexes.remove(&key);
            index.collection = next_name.to_string();
            indexes.insert(Self::index_key(&index.collection, &index.name), index);
        }

        let mut vector_indexes = self.vector_indexes.write();
        let keys: Vec<(String, String)> = vector_indexes
            .iter()
            .filter(|(_, record)| record.collection == current_name)
            .map(|(key, record)| (key.clone(), record.field.clone()))
            .collect::<Vec<_>>();

        for (key, field) in keys {
            if let Some(mut metadata) = vector_indexes.remove(&key) {
                metadata.collection = next_name.to_string();
                let next_key = Self::vector_index_key(&metadata.collection, &field);
                vector_indexes.insert(next_key, metadata);
            }
        }

        let mut cardinality = self.cardinality.write();
        if let Some(stats) = cardinality.remove(current_name) {
            cardinality.insert(next_name.to_string(), stats);
        }

        let mut rollups = self.rollups.write();
        for rollup in rollups.values_mut() {
            if rollup.source_collection == current_name {
                rollup.source_collection = next_name.to_string();
            }
        }
        let mut retention_policies = self.retention_policies.write();
        for policy in retention_policies.values_mut() {
            if policy.collection == current_name {
                policy.collection = next_name.to_string();
            }
        }
        self.bump_version();
    }

    pub fn rename_collection_field(&self, collection: &str, current_name: &str, next_name: &str) {
        let mut schemas = self.schemas.write();
        let Some(schema) = schemas.get_mut(collection) else {
            return;
        };

        let Some(field) = schema
            .fields
            .iter_mut()
            .find(|entry| entry.name.eq_ignore_ascii_case(current_name))
        else {
            return;
        };
        field.name = next_name.to_string();

        let mut constraints = self.constraints.write();
        if let Some(entries) = constraints.get_mut(collection) {
            for constraint in entries {
                if constraint.field.eq_ignore_ascii_case(current_name) {
                    constraint.field = next_name.to_string();
                }
                if let Some(check) = constraint.check.as_mut() {
                    if check.field.eq_ignore_ascii_case(current_name) {
                        check.field = next_name.to_string();
                    }
                }
            }
        }

        let mut indexes = self.indexes.write();
        for index in indexes
            .values_mut()
            .filter(|index| index.collection == collection)
        {
            let _ = index.rename_field(current_name, next_name);
        }

        let mut vector_indexes = self.vector_indexes.write();
        let keys = vector_indexes
            .iter()
            .filter(|(_, record)| record.collection == collection)
            .map(|(key, record)| {
                (
                    key.clone(),
                    record.field.clone(),
                    record.source_field.clone(),
                )
            })
            .collect::<Vec<_>>();
        for (key, field, source_field) in keys {
            let Some(mut record) = vector_indexes.remove(&key) else {
                continue;
            };
            let mut changed_key = false;
            if field.eq_ignore_ascii_case(current_name) {
                record.field = next_name.to_string();
                changed_key = true;
            }
            if source_field.eq_ignore_ascii_case(current_name) {
                record.source_field = next_name.to_string();
            }
            let next_key = if changed_key {
                Self::vector_index_key(collection, &record.field)
            } else {
                key
            };
            vector_indexes.insert(next_key, record);
        }

        let mut retention_policies = self.retention_policies.write();
        for policy in retention_policies
            .values_mut()
            .filter(|policy| policy.collection == collection)
        {
            if policy.timestamp_field.eq_ignore_ascii_case(current_name) {
                policy.timestamp_field = next_name.to_string();
            }
        }

        self.bump_version();
    }

    #[must_use]
    pub fn exists(&self, collection: &str) -> bool {
        let collections = self.collections.read();
        collections.contains_key(collection)
            || collections
                .keys()
                .any(|stored| name_matches(stored, collection))
    }

    #[must_use]
    pub fn relation_exists(&self, name: &str) -> bool {
        if self.exists(name) {
            return true;
        }

        if self
            .views
            .read()
            .keys()
            .any(|stored| name_matches(stored, name))
        {
            return true;
        }

        self.is_materialized_projection(name)
    }

    #[must_use]
    pub fn get_field_boost(&self, collection: &str, field: &str) -> Option<f32> {
        let schemas = self.schemas.read();
        schemas
            .get(collection)
            .and_then(|schema| schema.field(field))
            .and_then(|field| field.boost)
    }

    #[must_use]
    pub fn set_field_boost(&self, collection: &str, field: &str, boost: f32) -> bool {
        let mut schemas = self.schemas.write();
        let Some(schema) = schemas.get_mut(collection) else {
            return false;
        };

        let Some(field_meta) = schema.fields.iter_mut().find(|entry| entry.name == field) else {
            return false;
        };

        field_meta.boost = Some(boost);
        self.bump_version();
        true
    }

    #[must_use]
    pub fn text_fields(&self, collection: &str) -> Vec<String> {
        let schemas = self.schemas.read();
        schemas
            .get(collection)
            .map(|schema| {
                schema
                    .fields
                    .iter()
                    .filter(|field| field.is_indexed && field.data_type == DataType::Text)
                    .map(|field| field.name.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn register_vector_index(&self, record: VectorIndexRecord) {
        let mut indexes = self.vector_indexes.write();
        let key = Self::vector_index_key(&record.collection, &record.field);
        indexes.insert(key, record);
        self.bump_version();
    }

    #[must_use]
    pub fn get_cardinality_stats(&self, collection: &str) -> Option<CollectionCardinalityStats> {
        let cardinality = self.cardinality.read();
        cardinality.get(collection).cloned().or_else(|| {
            cardinality
                .iter()
                .find(|(stored, _)| name_matches(stored, collection))
                .map(|(_, stats)| stats.clone())
        })
    }

    #[must_use]
    pub fn cardinality_snapshot(&self) -> HashMap<String, CollectionCardinalityStats> {
        self.cardinality.read().clone()
    }

    pub fn set_cardinality_stats(&self, collection: &str, stats: CollectionCardinalityStats) {
        self.cardinality
            .write()
            .insert(collection.to_string(), stats);
        self.bump_version();
    }

    pub fn clear_cardinality_stats(&self, collection: &str) {
        let mut cardinality = self.cardinality.write();
        if cardinality.remove(collection).is_none() {
            let matching_key = cardinality
                .keys()
                .find(|stored| name_matches(stored, collection))
                .cloned();
            if let Some(key) = matching_key {
                cardinality.remove(&key);
            }
        }
        self.bump_version();
    }

    pub fn adjust_row_cardinality(&self, collection: &str, delta: i64) {
        let mut cardinality = self.cardinality.write();
        let stats = cardinality
            .entry(collection.to_string())
            .or_insert_with(|| CollectionCardinalityStats {
                hydrated: false,
                ..CollectionCardinalityStats::default()
            });
        if delta.is_positive() {
            stats.row_count = stats.row_count.saturating_add(delta.unsigned_abs());
        } else if delta.is_negative() {
            stats.row_count = stats.row_count.saturating_sub(delta.unsigned_abs());
        }
        self.bump_version();
    }

    pub fn set_index_cardinality(&self, collection: &str, key: String, cardinality: u64) {
        let mut cardinality_map = self.cardinality.write();
        let stats = cardinality_map
            .entry(collection.to_string())
            .or_insert_with(|| CollectionCardinalityStats {
                hydrated: false,
                ..CollectionCardinalityStats::default()
            });
        stats.set_index_cardinality(key, cardinality);
        self.bump_version();
    }

    pub fn remove_index_cardinality(&self, collection: &str, key: &str) {
        let mut cardinality = self.cardinality.write();
        let stored_key = if cardinality.contains_key(collection) {
            Some(collection.to_string())
        } else {
            cardinality
                .keys()
                .find(|stored| name_matches(stored, collection))
                .cloned()
        };
        if let Some(stats) = stored_key
            .as_deref()
            .and_then(|name| cardinality.get_mut(name))
        {
            stats.indexes.remove(key);
            self.bump_version();
        }
    }

    pub fn hydrate_cardinality_stats(
        &self,
        collection: &str,
        mut stats: CollectionCardinalityStats,
    ) {
        stats.hydrated = true;
        self.cardinality
            .write()
            .insert(collection.to_string(), stats);
        self.bump_version();
    }

    pub fn unregister_vector_index(&self, collection: &str, field: &str) {
        let mut indexes = self.vector_indexes.write();
        let key = Self::vector_index_key(collection, field);
        if indexes.remove(&key).is_none() {
            let matching_key = indexes
                .iter()
                .find(|(_, record)| {
                    name_matches(&record.collection, collection)
                        && record.field.eq_ignore_ascii_case(field)
                })
                .map(|(stored_key, _)| stored_key.clone());
            if let Some(stored_key) = matching_key {
                indexes.remove(&stored_key);
            }
        }
        self.bump_version();
    }

    #[must_use]
    pub fn get_vector_index(
        &self,
        collection: &str,
        vector_field: &str,
    ) -> Option<VectorIndexRecord> {
        let indexes = self.vector_indexes.read();
        let record = indexes
            .get(&Self::vector_index_key(collection, vector_field))
            .cloned()
            .or_else(|| {
                indexes
                    .values()
                    .find(|record| {
                        name_matches(&record.collection, collection)
                            && record.field.eq_ignore_ascii_case(vector_field)
                    })
                    .cloned()
            })?;
        if matches!(
            parse_name(collection),
            Ok(crate::catalog::ParsedName::Unqualified(_))
        ) {
            let mut display = record;
            display.collection = local_name(&display.collection);
            Some(display)
        } else {
            Some(record)
        }
    }

    #[must_use]
    pub fn list_vector_indexes(&self, collection: &str) -> Vec<VectorIndexRecord> {
        let indexes = self.vector_indexes.read();
        indexes
            .values()
            .filter(|record| name_matches(&record.collection, collection))
            .cloned()
            .collect()
    }

    pub fn clear_vector_indexes(&self, collection: &str) {
        let mut indexes = self.vector_indexes.write();
        indexes.retain(|_, value| !name_matches(&value.collection, collection));
        self.bump_version();
    }

    #[must_use]
    pub fn vector_index_key(collection: &str, field: &str) -> String {
        format!("{collection}:{field}")
    }

    fn index_key(collection: &str, name: &str) -> String {
        format!("{collection}:{name}")
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

    pub(crate) fn bump_version(&self) {
        self.version.fetch_add(1, Ordering::SeqCst);
    }
}

impl Default for Catalog {
    fn default() -> Self {
        Self::new()
    }
}

fn view_schema_to_collection_schema(name: &str, schema: &Schema) -> CollectionSchema {
    CollectionSchema {
        collection: name.to_string(),
        fields: schema
            .fields
            .iter()
            .map(|field| FieldMeta {
                name: field.name.clone(),
                data_type: field.data_type.clone(),
                is_indexed: false,
                boost: None,
            })
            .collect(),
    }
}
