use super::auth::hash_password;
use super::{Cassie, CassieError, Instant, normalize_role_name, RoleMeta, current_time_millis};

impl Cassie {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn hydrate_catalog(&self) -> Result<(), CassieError> {
        let started_at = Instant::now();
        self.catalog.clear();
        self.invalidate_plan_cache();

        let namespaces = self.midge.list_namespaces();
        self.runtime.record_storage_access("schema", false, true);
        for namespace in namespaces {
            self.catalog.register_namespace(&namespace, None);
        }

        let mut collections = self.midge.list_collections();
        self.runtime.record_storage_access("schema", false, true);
        if collections.is_empty() {
            collections = self.midge.list_collections_from_schema();
            self.runtime.record_storage_access("schema", false, true);
        }

        for name in collections {
            self.runtime.record_storage_access("schema", false, true);
            if let Some(schema) = self.midge.collection_schema(&name) {
                let constraints = self.midge.load_constraints(&name).map_err(|error| {
                    self.runtime.record_storage_access("schema", false, false);
                    CassieError::Storage(format!(
                        "load constraints for collection '{name}': {error}"
                    ))
                })?;
                let metadata = self.midge.collection_metadata(&name).map_err(|error| {
                    self.runtime.record_storage_access("schema", false, false);
                    CassieError::Storage(format!("load collection metadata for '{name}': {error}"))
                })?;
                self.runtime.record_storage_access("schema", false, true);
                self.catalog.register_collection_meta_with_constraints(
                    metadata.unwrap_or_else(|| crate::catalog::CollectionMeta::new(&name, None)),
                    schema
                        .fields
                        .into_iter()
                        .map(|field| (field.name, field.data_type))
                        .collect(),
                    constraints,
                );
                let projection_metadata = self
                    .midge
                    .projection_metadata(&name)?
                    .unwrap_or_else(|| crate::catalog::ProjectionMeta::new(&name, 1));
                self.catalog
                    .register_projection_metadata(projection_metadata);
            }
        }

        let projection_metadata = self.midge.list_projection_metadata().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list projection metadata: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for metadata in projection_metadata {
            if metadata.kind == crate::catalog::ProjectionKind::Materialized {
                self.catalog.register_projection_metadata(metadata);
            }
        }

        let comparison_reports =
            self.midge
                .list_projection_comparison_reports()
                .map_err(|error| {
                    self.runtime.record_storage_access("schema", false, false);
                    CassieError::Storage(format!("list projection comparison reports: {error}"))
                })?;
        self.runtime.record_storage_access("schema", false, true);
        for report in comparison_reports {
            self.catalog.register_projection_comparison_report(report);
        }

        let consistency_reports =
            self.midge
                .list_projection_consistency_reports()
                .map_err(|error| {
                    self.runtime.record_storage_access("schema", false, false);
                    CassieError::Storage(format!("list projection consistency reports: {error}"))
                })?;
        self.runtime.record_storage_access("schema", false, true);
        for report in consistency_reports {
            self.catalog.register_projection_consistency_report(report);
        }

        let repair_reports = self
            .midge
            .list_projection_repair_reports()
            .map_err(|error| {
                self.runtime.record_storage_access("schema", false, false);
                CassieError::Storage(format!("list projection repair reports: {error}"))
            })?;
        self.runtime.record_storage_access("schema", false, true);
        for report in repair_reports {
            self.catalog.register_projection_repair_report(report);
        }

        let assignments = self.midge.list_operational_assignments().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list operational assignments: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for assignment in assignments {
            self.catalog.register_operational_assignment(assignment);
        }

        let indexes = self.midge.list_vector_indexes().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list vector indexes: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for index in indexes {
            self.catalog.register_vector_index(index.clone());
            self.midge.rebuild_normalized_vectors_for_index(&index)?;
        }

        let indexes = self.midge.list_indexes().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list indexes: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for index in indexes {
            self.catalog.register_index(index);
        }

        let graphs = self.midge.list_graphs().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list graphs: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for graph in graphs {
            self.catalog.register_graph(graph);
        }

        let sequences = self.midge.list_sequences().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list sequences: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for sequence in sequences {
            self.catalog.register_sequence(sequence);
        }

        for collection in self.catalog.list_collections() {
            self.hydrate_cardinality_stats(&collection.name)?;
        }

        let functions = self.midge.list_functions().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list functions: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for metadata in functions {
            self.catalog.register_function(metadata);
        }

        let procedures = self.midge.list_procedures().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list procedures: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for metadata in procedures {
            self.catalog.register_procedure(metadata);
        }

        let views = self.midge.list_views().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list views: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for metadata in views {
            self.catalog.register_view(metadata);
        }

        let rollups = self.midge.list_rollups().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list rollups: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for metadata in rollups {
            self.catalog.register_rollup(metadata);
        }

        let retention_policies = self.midge.list_retention_policies().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list retention policies: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for metadata in retention_policies {
            self.catalog.register_retention_policy(metadata);
        }

        self.hydrate_roles()?;
        self.runtime.record_catalog_hydration(started_at.elapsed());
        Ok(())
    }

    fn hydrate_roles(&self) -> Result<(), CassieError> {
        let mut roles = self.midge.list_roles().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list roles: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);

        let admin_name = normalize_role_name(&self.auth_user);
        if !roles.iter().any(|role| role.name == admin_name) {
            let password_hash = if self.auth_password.is_empty() {
                None
            } else {
                Some(hash_password(&self.auth_password)?)
            };
            let role = RoleMeta::bootstrap_admin(&self.auth_user, password_hash);
            self.midge.put_role(role.clone()).map_err(|error| {
                self.runtime.record_storage_access("schema", false, false);
                CassieError::Storage(format!("create bootstrap role: {error}"))
            })?;
            self.runtime.record_storage_access("schema", false, true);
            roles.push(role);
        }

        for role in roles {
            self.catalog.register_role(role);
        }

        Ok(())
    }

    fn hydrate_cardinality_stats(&self, collection: &str) -> Result<(), CassieError> {
        self.runtime.record_cardinality_read();
        match self.midge.get_cardinality_stats(collection) {
            Ok(Some(stats)) if stats.hydrated => {
                self.catalog.hydrate_cardinality_stats(collection, stats);
                Ok(())
            }
            Ok(_) => {
                self.runtime.record_cardinality_unavailable();
                let stats = self
                    .midge
                    .rebuild_cardinality_stats_for_collection(collection)
                    .map_err(|error| {
                        CassieError::Storage(format!(
                            "rebuild cardinality stats for collection '{collection}': {error}"
                        ))
                    })?;
                self.runtime.record_cardinality_rebuild();
                self.runtime.record_cardinality_write();
                self.catalog.hydrate_cardinality_stats(collection, stats);
                Ok(())
            }
            Err(error) => Err(CassieError::Storage(format!(
                "load cardinality stats for collection '{collection}': {error}"
            ))),
        }
    }

    pub(crate) fn refresh_cardinality_stats(&self, collection: &str) -> Result<(), CassieError> {
        let stats = self
            .midge
            .rebuild_cardinality_stats_for_collection(collection)
            .map_err(|error| {
                CassieError::Storage(format!(
                    "rebuild cardinality stats for collection '{collection}': {error}"
                ))
            })?;
        self.runtime.record_cardinality_rebuild();
        self.runtime.record_cardinality_write();
        self.catalog.hydrate_cardinality_stats(collection, stats);
        Ok(())
    }

    pub(crate) fn hydrate_runtime_feedback(&self) -> Result<(), CassieError> {
        let records = self
            .midge
            .list_runtime_feedback_records()
            .map_err(|error| {
                self.runtime.record_storage_access("schema", false, false);
                CassieError::Storage(format!("list operator feedback: {error}"))
            })?;
        self.runtime.record_storage_access("schema", false, true);

        let current_epoch = self.runtime.schema_epoch();
        let effective_epoch = if current_epoch == 0 {
            records
                .iter()
                .map(|(key, _record)| key.schema_epoch)
                .max()
                .unwrap_or(current_epoch)
        } else {
            current_epoch
        };
        if effective_epoch != current_epoch {
            self.runtime.set_schema_epoch(effective_epoch);
        }
        let now_ms = current_time_millis();
        let ttl_ms = self
            .runtime
            .limits()
            .feedback_ttl_seconds
            .saturating_mul(1_000);
        let hydrated = records
            .into_iter()
            .filter(|(key, record)| {
                key.schema_epoch == effective_epoch
                    && (ttl_ms == 0 || now_ms.saturating_sub(record.last_seen_ms) <= ttl_ms)
            })
            .collect::<Vec<_>>();
        self.runtime.replace_feedback_records(hydrated);
        self.persist_runtime_feedback();
        Ok(())
    }
}
