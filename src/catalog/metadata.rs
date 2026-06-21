use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;

use crate::catalog::{
    normalize_role_name, CollectionCardinalityStats, CollectionMeta, CollectionSchema,
    FieldConstraint, FieldMeta, FunctionMeta, IndexKind, IndexMeta, NamespaceMeta, ProcedureMeta,
    ProjectionMeta, RetentionPolicyMeta, RoleMeta, RollupMeta, ViewMeta,
};
use crate::embeddings::VectorIndexRecord;
use crate::types::{DataType, Schema};

#[derive(Debug, Clone)]
pub struct Catalog {
    pub collections: Arc<RwLock<HashMap<String, CollectionMeta>>>,
    pub namespaces: Arc<RwLock<HashMap<String, NamespaceMeta>>>,
    pub schemas: Arc<RwLock<HashMap<String, CollectionSchema>>>,
    pub projections: Arc<RwLock<HashMap<String, ProjectionMeta>>>,
    pub constraints: Arc<RwLock<HashMap<String, Vec<FieldConstraint>>>>,
    pub indexes: Arc<RwLock<HashMap<String, IndexMeta>>>,
    pub functions: Arc<RwLock<HashMap<String, FunctionMeta>>>,
    pub procedures: Arc<RwLock<HashMap<String, ProcedureMeta>>>,
    pub views: Arc<RwLock<HashMap<String, ViewMeta>>>,
    pub roles: Arc<RwLock<HashMap<String, RoleMeta>>>,
    pub rollups: Arc<RwLock<HashMap<String, RollupMeta>>>,
    pub retention_policies: Arc<RwLock<HashMap<String, RetentionPolicyMeta>>>,
    pub vector_indexes: Arc<RwLock<HashMap<String, VectorIndexRecord>>>,
    pub cardinality: Arc<RwLock<HashMap<String, CollectionCardinalityStats>>>,
    version: Arc<AtomicU64>,
}

impl Catalog {
    pub fn new() -> Self {
        Self {
            collections: Arc::new(RwLock::new(HashMap::new())),
            namespaces: Arc::new(RwLock::new(HashMap::new())),
            schemas: Arc::new(RwLock::new(HashMap::new())),
            projections: Arc::new(RwLock::new(HashMap::new())),
            constraints: Arc::new(RwLock::new(HashMap::new())),
            indexes: Arc::new(RwLock::new(HashMap::new())),
            functions: Arc::new(RwLock::new(HashMap::new())),
            procedures: Arc::new(RwLock::new(HashMap::new())),
            views: Arc::new(RwLock::new(HashMap::new())),
            roles: Arc::new(RwLock::new(HashMap::new())),
            rollups: Arc::new(RwLock::new(HashMap::new())),
            retention_policies: Arc::new(RwLock::new(HashMap::new())),
            vector_indexes: Arc::new(RwLock::new(HashMap::new())),
            cardinality: Arc::new(RwLock::new(HashMap::new())),
            version: Arc::new(AtomicU64::new(0)),
        }
    }

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
        let mut collections = self.collections.write();
        collections.insert(name.to_string(), CollectionMeta::new(name, None));

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
            name.to_string(),
            CollectionSchema {
                collection: name.to_string(),
                fields,
            },
        );

        let normalized = constraints
            .into_iter()
            .filter(Self::is_constraint_populated)
            .collect::<Vec<_>>();
        self.constraints
            .write()
            .insert(name.to_string(), normalized);
        self.register_projection_metadata(ProjectionMeta::new(name, 1));
        self.cardinality.write().insert(
            name.to_string(),
            CollectionCardinalityStats {
                hydrated: false,
                ..CollectionCardinalityStats::default()
            },
        );
        self.bump_version();
    }

    pub fn list_collections(&self) -> Vec<CollectionMeta> {
        let collections = self.collections.read();
        let mut out = collections.values().cloned().collect::<Vec<_>>();
        out.sort_by_key(|entry| entry.name.to_ascii_lowercase());
        out
    }

    pub fn register_projection_metadata(&self, metadata: ProjectionMeta) {
        self.projections
            .write()
            .insert(metadata.collection.clone(), metadata);
        self.bump_version();
    }

    pub fn get_projection_metadata(&self, collection: &str) -> Option<ProjectionMeta> {
        self.projections.read().get(collection).cloned()
    }

    pub fn register_namespace(&self, name: &str, description: Option<String>) {
        let mut namespaces = self.namespaces.write();
        namespaces.insert(name.to_string(), NamespaceMeta::new(name, description));
        self.bump_version();
    }

    pub fn unregister_namespace(&self, name: &str) {
        self.namespaces.write().remove(name);
        self.bump_version();
    }

    pub fn rename_namespace(&self, current_name: &str, next_name: &str) {
        let mut namespaces = self.namespaces.write();
        let Some(namespace) = namespaces.remove(current_name) else {
            return;
        };
        let description = namespace.description;
        namespaces.insert(
            next_name.to_string(),
            NamespaceMeta::new(next_name, description),
        );
        self.bump_version();
    }

    pub fn list_namespaces(&self) -> Vec<NamespaceMeta> {
        let namespaces = self.namespaces.read();
        let mut out = namespaces.values().cloned().collect::<Vec<_>>();
        out.sort_by_key(|entry| entry.name.to_ascii_lowercase());
        out
    }

    pub fn register_function(&self, metadata: FunctionMeta) {
        let mut functions = self.functions.write();
        functions.insert(metadata.name.to_ascii_lowercase(), metadata);
        self.bump_version();
    }

    pub fn unregister_function(&self, name: &str) {
        self.functions.write().remove(&name.to_ascii_lowercase());
        self.bump_version();
    }

    pub fn get_function(&self, name: &str) -> Option<FunctionMeta> {
        self.functions
            .read()
            .get(&name.to_ascii_lowercase())
            .cloned()
    }

    pub fn list_functions(&self) -> Vec<FunctionMeta> {
        let mut out = self.functions.read().values().cloned().collect::<Vec<_>>();
        out.sort_by_key(|function| function.name.to_ascii_lowercase());
        out
    }

    pub fn register_view(&self, metadata: ViewMeta) {
        let mut views = self.views.write();
        views.insert(metadata.name.clone(), metadata);
        self.bump_version();
    }

    pub fn unregister_view(&self, name: &str) {
        self.views.write().remove(name);
        self.bump_version();
    }

    pub fn get_view(&self, name: &str) -> Option<ViewMeta> {
        self.views.read().get(name).cloned()
    }

    pub fn list_views(&self) -> Vec<ViewMeta> {
        let mut out = self.views.read().values().cloned().collect::<Vec<_>>();
        out.sort_by_key(|view| view.name.to_ascii_lowercase());
        out
    }

    pub fn register_procedure(&self, metadata: ProcedureMeta) {
        let mut procedures = self.procedures.write();
        procedures.insert(metadata.name.to_ascii_lowercase(), metadata);
        self.bump_version();
    }

    pub fn unregister_procedure(&self, name: &str) {
        self.procedures.write().remove(&name.to_ascii_lowercase());
        self.bump_version();
    }

    pub fn get_procedure(&self, name: &str) -> Option<ProcedureMeta> {
        self.procedures
            .read()
            .get(&name.to_ascii_lowercase())
            .cloned()
    }

    pub fn list_procedures(&self) -> Vec<ProcedureMeta> {
        let mut out = self.procedures.read().values().cloned().collect::<Vec<_>>();
        out.sort_by_key(|procedure| procedure.name.to_ascii_lowercase());
        out
    }

    pub fn register_role(&self, metadata: RoleMeta) {
        let mut roles = self.roles.write();
        roles.insert(normalize_role_name(&metadata.name), metadata);
        self.bump_version();
    }

    pub fn unregister_role(&self, name: &str) {
        self.roles.write().remove(&normalize_role_name(name));
        self.bump_version();
    }

    pub fn get_role(&self, name: &str) -> Option<RoleMeta> {
        self.roles.read().get(&normalize_role_name(name)).cloned()
    }

    pub fn list_roles(&self) -> Vec<RoleMeta> {
        let mut out = self.roles.read().values().cloned().collect::<Vec<_>>();
        out.sort_by_key(|role| role.name.to_ascii_lowercase());
        out
    }

    pub fn register_rollup(&self, metadata: RollupMeta) {
        self.rollups
            .write()
            .insert(metadata.name.to_ascii_lowercase(), metadata);
        self.bump_version();
    }

    pub fn unregister_rollup(&self, name: &str) {
        self.rollups.write().remove(&name.to_ascii_lowercase());
        self.bump_version();
    }

    pub fn get_rollup(&self, name: &str) -> Option<RollupMeta> {
        self.rollups.read().get(&name.to_ascii_lowercase()).cloned()
    }

    pub fn list_rollups(&self) -> Vec<RollupMeta> {
        let mut out = self.rollups.read().values().cloned().collect::<Vec<_>>();
        out.sort_by_key(|rollup| rollup.name.to_ascii_lowercase());
        out
    }

    pub fn list_rollups_for_source(&self, source_collection: &str) -> Vec<RollupMeta> {
        let mut out = self
            .rollups
            .read()
            .values()
            .filter(|rollup| rollup.source_collection == source_collection)
            .cloned()
            .collect::<Vec<_>>();
        out.sort_by_key(|rollup| rollup.name.to_ascii_lowercase());
        out
    }

    pub fn register_retention_policy(&self, metadata: RetentionPolicyMeta) {
        self.retention_policies
            .write()
            .insert(metadata.name.to_ascii_lowercase(), metadata);
        self.bump_version();
    }

    pub fn unregister_retention_policy(&self, name: &str) {
        self.retention_policies
            .write()
            .remove(&name.to_ascii_lowercase());
        self.bump_version();
    }

    pub fn get_retention_policy(&self, name: &str) -> Option<RetentionPolicyMeta> {
        self.retention_policies
            .read()
            .get(&name.to_ascii_lowercase())
            .cloned()
    }

    pub fn list_retention_policies(&self) -> Vec<RetentionPolicyMeta> {
        let mut out = self
            .retention_policies
            .read()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        out.sort_by_key(|policy| policy.name.to_ascii_lowercase());
        out
    }

    pub fn namespace_exists(&self, namespace: &str) -> bool {
        self.namespaces.read().contains_key(namespace)
    }

    pub fn clear(&self) {
        self.collections.write().clear();
        self.namespaces.write().clear();
        self.schemas.write().clear();
        self.projections.write().clear();
        self.constraints.write().clear();
        self.functions.write().clear();
        self.procedures.write().clear();
        self.views.write().clear();
        self.roles.write().clear();
        self.rollups.write().clear();
        self.retention_policies.write().clear();
        self.indexes.write().clear();
        self.vector_indexes.write().clear();
        self.cardinality.write().clear();
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
        self.bump_version();
    }

    pub fn get_constraints(&self, collection: &str) -> Vec<FieldConstraint> {
        self.constraints
            .read()
            .get(collection)
            .cloned()
            .unwrap_or_default()
    }

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
        self.indexes
            .write()
            .remove(&Self::index_key(collection, name));
        self.bump_version();
    }

    pub fn get_index(&self, collection: &str, name: &str) -> Option<IndexMeta> {
        let indexes = self.indexes.read();
        indexes.get(&Self::index_key(collection, name)).cloned()
    }

    pub fn list_indexes(&self, collection: &str) -> Vec<IndexMeta> {
        let indexes = self.indexes.read();
        let mut out = indexes
            .values()
            .filter(|index| index.collection == collection)
            .cloned()
            .collect::<Vec<_>>();
        out.sort_by_key(|index| index.name.to_ascii_lowercase());
        out
    }

    pub fn get_schema(&self, collection: &str) -> Option<CollectionSchema> {
        let schemas = self.schemas.read();
        if let Some(schema) = schemas.get(collection).cloned() {
            return Some(schema);
        }
        drop(schemas);

        self.views
            .read()
            .get(collection)
            .map(|view| view_schema_to_collection_schema(&view.name, &view.schema))
    }

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
        collections.remove(current_name);
        collections.insert(next_name.to_string(), CollectionMeta::new(next_name, None));

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

    pub fn exists(&self, collection: &str) -> bool {
        self.collections.read().contains_key(collection)
    }

    pub fn relation_exists(&self, name: &str) -> bool {
        if self.collections.read().contains_key(name) {
            return true;
        }

        self.views.read().contains_key(name)
    }

    pub fn get_field_boost(&self, collection: &str, field: &str) -> Option<f32> {
        let schemas = self.schemas.read();
        schemas
            .get(collection)
            .and_then(|schema| schema.field(field))
            .and_then(|field| field.boost)
    }

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

    pub fn get_cardinality_stats(&self, collection: &str) -> Option<CollectionCardinalityStats> {
        self.cardinality.read().get(collection).cloned()
    }

    pub fn set_cardinality_stats(&self, collection: &str, stats: CollectionCardinalityStats) {
        self.cardinality
            .write()
            .insert(collection.to_string(), stats);
        self.bump_version();
    }

    pub fn clear_cardinality_stats(&self, collection: &str) {
        self.cardinality.write().remove(collection);
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
            stats.row_count = stats.row_count.saturating_add(delta as u64);
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
        if let Some(stats) = self.cardinality.write().get_mut(collection) {
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
        self.vector_indexes
            .write()
            .remove(&Self::vector_index_key(collection, field));
        self.bump_version();
    }

    pub fn get_vector_index(
        &self,
        collection: &str,
        vector_field: &str,
    ) -> Option<VectorIndexRecord> {
        let indexes = self.vector_indexes.read();
        indexes
            .get(&Self::vector_index_key(collection, vector_field))
            .cloned()
    }

    pub fn list_vector_indexes(&self, collection: &str) -> Vec<VectorIndexRecord> {
        let indexes = self.vector_indexes.read();
        indexes
            .values()
            .filter(|record| record.collection == collection)
            .cloned()
            .collect()
    }

    pub fn clear_vector_indexes(&self, collection: &str) {
        let mut indexes = self.vector_indexes.write();
        indexes.retain(|_, value| value.collection != collection);
        self.bump_version();
    }

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
            || constraint.check.is_some()
            || constraint.references_table.is_some()
    }

    fn bump_version(&self) {
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
