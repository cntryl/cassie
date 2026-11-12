use super::auth::verify_password;
use super::{current_time_millis, normalize_role_name, Cassie, CassieError, Instant, RoleMeta};
use crate::catalog::RoleGrantTarget;
use std::collections::BTreeSet;

impl Cassie {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn hydrate_catalog(&self) -> Result<(), CassieError> {
        let started_at = Instant::now();
        self.reset_catalog_hydration();
        self.hydrate_databases()?;
        self.hydrate_namespaces();
        self.hydrate_collections()?;
        self.hydrate_projection_state()?;
        self.hydrate_operational_metadata()?;
        self.hydrate_collection_cardinality_stats()?;
        self.hydrate_programmable_metadata()?;
        self.hydrate_time_series_metadata()?;
        self.hydrate_maintenance_debt()?;
        self.hydrate_roles()?;
        self.hydrate_role_object_grants()?;
        self.hydrate_object_comments()?;
        self.runtime.record_catalog_hydration(started_at.elapsed());
        Ok(())
    }

    fn reset_catalog_hydration(&self) {
        self.catalog.clear();
        self.invalidate_plan_cache();
    }

    fn hydrate_databases(&self) -> Result<(), CassieError> {
        let databases = self.midge.list_databases().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list databases: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for database in databases {
            self.catalog.register_database_metadata(database);
        }
        Ok(())
    }

    fn hydrate_namespaces(&self) {
        let namespaces = self.midge.list_namespaces_canonical();
        self.runtime.record_storage_access("schema", false, true);
        for namespace in namespaces {
            match self.midge.namespace_metadata(&namespace) {
                Ok(Some(mut metadata)) => {
                    metadata.name = namespace;
                    self.catalog.register_namespace_meta(metadata);
                }
                _ => self.catalog.register_namespace(&namespace, None),
            }
        }
    }

    fn hydrate_collections(&self) -> Result<(), CassieError> {
        let mut collections = self.midge.list_collections();
        self.runtime.record_storage_access("schema", false, true);
        if collections.is_empty() {
            collections = self.midge.list_collections_from_schema();
            self.runtime.record_storage_access("schema", false, true);
        }

        for name in collections {
            self.hydrate_collection(&name)?;
        }
        Ok(())
    }

    fn hydrate_collection(&self, name: &str) -> Result<(), CassieError> {
        self.runtime.record_storage_access("schema", false, true);
        let Some(schema) = self.midge.collection_schema(name) else {
            return Ok(());
        };
        let constraints = self.midge.load_constraints(name).map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("load constraints for collection '{name}': {error}"))
        })?;
        let metadata = self.midge.collection_metadata(name).map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("load collection metadata for '{name}': {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        self.catalog.register_collection_meta_with_constraints(
            metadata.unwrap_or_else(|| crate::catalog::CollectionMeta::new(name, None)),
            schema
                .fields
                .into_iter()
                .map(|field| (field.name, field.data_type))
                .collect(),
            constraints,
        );
        let projection_metadata = self
            .midge
            .projection_metadata(name)?
            .unwrap_or_else(|| crate::catalog::ProjectionMeta::new(name, 1));
        self.catalog
            .register_projection_metadata(projection_metadata);
        Ok(())
    }

    fn hydrate_projection_state(&self) -> Result<(), CassieError> {
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
        Ok(())
    }

    fn hydrate_operational_metadata(&self) -> Result<(), CassieError> {
        let assignments = self.midge.list_operational_assignments().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list operational assignments: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for assignment in assignments {
            self.catalog.register_operational_assignment(assignment);
        }

        let indexes = self
            .midge
            .list_vector_indexes_canonical()
            .map_err(|error| {
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
        for mut index in indexes {
            index.clear_storage_ids();
            self.catalog.register_index(index);
        }

        let graphs = self.midge.list_graphs().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list graphs: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for mut graph in graphs {
            graph.name = self.midge.canonical_collection_name(&graph.name);
            graph.node_collection = self.midge.canonical_collection_name(&graph.node_collection);
            graph.edge_collection = self.midge.canonical_collection_name(&graph.edge_collection);
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
        Ok(())
    }

    fn hydrate_collection_cardinality_stats(&self) -> Result<(), CassieError> {
        for collection in self.catalog.list_collections_canonical() {
            self.hydrate_cardinality_stats(&collection.name)?;
        }
        Ok(())
    }

    fn hydrate_programmable_metadata(&self) -> Result<(), CassieError> {
        let extensions = self.midge.list_extensions().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list extension metadata: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for metadata in extensions {
            self.catalog.register_extension(metadata);
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
        Ok(())
    }

    fn hydrate_time_series_metadata(&self) -> Result<(), CassieError> {
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
        Ok(())
    }

    fn hydrate_maintenance_debt(&self) -> Result<(), CassieError> {
        let debts = self.midge.list_maintenance_debt().map_err(|error| {
            self.runtime.record_storage_access("data", false, false);
            CassieError::Storage(format!("list maintenance debt: {error}"))
        })?;
        self.runtime.record_storage_access("data", false, true);
        for debt in debts {
            self.catalog
                .register_maintenance_debt(crate::catalog::MaintenanceDebtMeta::new(
                    debt.collection,
                    debt.artifact,
                    debt.target_generation,
                    debt.retry_count,
                    debt.last_error,
                ));
        }
        Ok(())
    }

    fn hydrate_roles(&self) -> Result<(), CassieError> {
        let mut roles = self.midge.list_roles().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list roles: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);

        let admin_name = normalize_role_name(&self.auth_user);
        if let Some(role) = roles.iter_mut().find(|role| role.name == admin_name) {
            self.reconcile_bootstrap_role_password(role)?;
        } else {
            let password_hash = self
                .initialize_bootstrap_password_hash()?
                .map(str::to_owned);
            let role = RoleMeta::bootstrap_admin(&self.auth_user, password_hash);
            self.midge.put_role(&role).map_err(|error| {
                self.runtime.record_storage_access("schema", false, false);
                CassieError::Storage(format!("create bootstrap role: {error}"))
            })?;
            self.runtime.record_storage_access("schema", false, true);
            roles.push(role);
        }
        for role in roles.iter_mut().filter(|role| !role.is_admin) {
            if role.database_grants.is_none() {
                role.grant_database(&self.default_database);
                self.persist_migrated_role(role)?;
            }
        }

        for role in roles {
            self.catalog.register_role(role);
        }

        self.hydrate_role_object_grants()
    }

    fn hydrate_role_object_grants(&self) -> Result<(), CassieError> {
        let grants = self.midge.list_role_object_grants().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list role object grants: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        let mut role_ids = HashSet::new();
        for role in self.catalog.list_roles() {
            role_ids.insert(role.object_id);
        }
        let mut schema_ids_by_family = HashMap::<String, HashSet<u64>>::new();
        for namespace in self.catalog.list_namespaces() {
            let database = crate::catalog::schema_database_name(&namespace.name)
                .unwrap_or_else(|| self.default_database.clone());
            if let Some(database) = self.catalog.get_database(&database) {
                schema_ids_by_family
                    .entry(database.physical_family)
                    .or_default()
                    .insert(namespace.object_id);
            }
        }
        let mut table_ids_by_family = HashMap::<String, HashSet<u64>>::new();
        for collection in self.catalog.list_collections() {
            let database = crate::catalog::relation_database_name(&collection.name)
                .unwrap_or_else(|| self.default_database.clone());
            if let Some(database) = self.catalog.get_database(&database) {
                table_ids_by_family
                    .entry(database.physical_family)
                    .or_default()
                    .insert(collection.storage_id);
            }
        }
        let mut stale_grants: Vec<(String, Vec<crate::catalog::RoleGrantTarget>)> = Vec::new();
        for grant in grants {
            let target_is_live = match &grant.target {
                crate::catalog::RoleGrantTarget::Schema(object_id) => schema_ids_by_family
                    .get(&grant.database_family)
                    .is_some_and(|ids| ids.contains(object_id)),
                crate::catalog::RoleGrantTarget::Table(object_id) => table_ids_by_family
                    .get(&grant.database_family)
                    .is_some_and(|ids| ids.contains(object_id)),
            };
            if role_ids.contains(&grant.role_id) && target_is_live {
                self.catalog.register_role_object_grant(grant);
                continue;
            }
            if let Some((_, targets)) = stale_grants
                .iter_mut()
                .find(|(family, _)| family == &grant.database_family)
            {
                if !targets.contains(&grant.target) {
                    targets.push(grant.target);
                }
            } else {
                stale_grants.push((grant.database_family, vec![grant.target]));
            }
        }
        for (database_family, targets) in stale_grants {
            match self
                .midge
                .delete_role_object_grants_for_targets(&database_family, &targets)
            {
                Ok(_) => self.runtime.record_storage_access("schema", true, true),
                Err(error) => {
                    self.runtime.record_storage_access("schema", true, false);
                    tracing::warn!(
                        database_family,
                        target_count = targets.len(),
                        %error,
                        "could not reconcile inert role grants during startup"
                    );
                }
            }
        }

        Ok(())
    }

    fn hydrate_object_comments(&self) -> Result<(), CassieError> {
        let comments = self.midge.list_object_comments().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list object comments: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for comment in comments {
            self.catalog.register_object_comment(comment);
        }
        Ok(())
    }

    fn hydrate_role_object_grants(&self) -> Result<(), CassieError> {
        let grants = self.midge.list_role_object_grants().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list role object grants: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);

        let role_ids = self
            .catalog
            .list_roles()
            .into_iter()
            .map(|role| role.object_id)
            .collect::<BTreeSet<_>>();
        let database_names = self
            .catalog
            .list_databases()
            .into_iter()
            .map(|database| (database.physical_family, database.name))
            .collect::<std::collections::HashMap<_, _>>();
        let namespaces = self.catalog.list_namespaces();
        let collections = self.catalog.list_collections_canonical();
        let mut stale_roles = BTreeSet::new();
        let mut stale_targets = Vec::<(String, RoleGrantTarget)>::new();

        for grant in grants {
            if !role_ids.contains(&grant.role_id) {
                stale_roles.insert(grant.role_id);
                continue;
            }
            let database_name = database_names.get(&grant.database_family);
            let target_exists = match (&grant.target, database_name) {
                (RoleGrantTarget::Schema(object_id), Some(database)) => namespaces.iter().any(
                    |namespace| {
                        namespace.object_id == *object_id
                            && crate::catalog::schema_database_name(&namespace.name)
                                .is_some_and(|name| name.eq_ignore_ascii_case(database))
                    },
                ),
                (RoleGrantTarget::Table(object_id), Some(database)) => collections.iter().any(
                    |collection| {
                        collection.storage_id == *object_id
                            && crate::catalog::relation_database_name(&collection.name)
                                .is_some_and(|name| name.eq_ignore_ascii_case(database))
                    },
                ),
                (_, None) => false,
            };
            if target_exists {
                self.catalog.register_role_object_grant(grant);
            } else if !stale_targets
                .iter()
                .any(|(family, target)| family == &grant.database_family && target == &grant.target)
            {
                stale_targets.push((grant.database_family, grant.target));
            }
        }

        for role_id in stale_roles {
            self.midge
                .delete_role_object_grants_for_role(role_id)
                .map_err(|error| {
                    self.runtime.record_storage_access("schema", false, false);
                    CassieError::Storage(format!("remove stale role object grants: {error}"))
                })?;
            self.runtime.record_storage_access("schema", false, true);
        }
        for (database_family, target) in stale_targets {
            self.midge
                .delete_role_object_grants_for_targets(&database_family, &[target])
                .map_err(|error| {
                    self.runtime.record_storage_access("schema", false, false);
                    CassieError::Storage(format!("remove stale target grants: {error}"))
                })?;
            self.runtime.record_storage_access("schema", false, true);
        }
        Ok(())
    }

    fn persist_migrated_role(&self, role: &RoleMeta) -> Result<(), CassieError> {
        self.midge.put_role(role).map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("migrate role database grants: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        Ok(())
    }

    fn reconcile_bootstrap_role_password(&self, role: &mut RoleMeta) -> Result<(), CassieError> {
        let password_matches = match role.password_hash.as_deref() {
            Some(hash) if !self.auth_password.is_empty() => {
                verify_password(hash, &self.auth_password).unwrap_or(false)
            }
            None => self.auth_password.is_empty(),
            Some(_) => false,
        };
        if password_matches {
            if let Some(hash) = role.password_hash.as_deref() {
                self.remember_bootstrap_password_hash(hash);
            }
            return Ok(());
        }

        role.password_hash = self
            .initialize_bootstrap_password_hash()?
            .map(str::to_owned);
        self.midge.put_role(role).map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("rotate bootstrap role credentials: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
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
