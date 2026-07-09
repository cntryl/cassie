use super::Catalog;
use crate::catalog::{
    name_matches, normalize_role_name, FunctionMeta, GraphMeta, NamespaceMeta, ProcedureMeta,
    ProjectionKind, ProjectionMeta, RetentionPolicyMeta, RoleMeta, RollupMeta, ViewMeta,
};

impl Catalog {
    pub fn register_graph(&self, metadata: GraphMeta) {
        self.graphs
            .write()
            .insert(metadata.name.to_ascii_lowercase(), metadata);
        self.bump_version();
    }

    #[must_use]
    pub fn get_graph(&self, name: &str) -> Option<GraphMeta> {
        let graphs = self.graphs.read();
        graphs
            .get(&name.to_ascii_lowercase())
            .cloned()
            .or_else(|| {
                graphs
                    .values()
                    .find(|graph| name_matches(&graph.name, name))
                    .cloned()
            })
    }

    #[must_use]
    pub fn graph_exists(&self, name: &str) -> bool {
        self.graphs.read().contains_key(&name.to_ascii_lowercase())
    }

    #[must_use]
    pub fn graph_for_edge_collection(&self, collection: &str) -> Option<GraphMeta> {
        self.graphs
            .read()
            .values()
            .find(|graph| graph.edge_collection.eq_ignore_ascii_case(collection))
            .cloned()
    }

    #[must_use]
    pub fn list_graphs(&self) -> Vec<GraphMeta> {
        let mut out = self.graphs.read().values().cloned().collect::<Vec<_>>();
        out.sort_by_key(|graph| graph.name.to_ascii_lowercase());
        out
    }

    pub fn register_projection_metadata(&self, metadata: ProjectionMeta) {
        self.projections
            .write()
            .insert(metadata.collection.clone(), metadata);
        self.bump_version();
    }

    #[must_use]
    pub fn get_projection_metadata(&self, collection: &str) -> Option<ProjectionMeta> {
        let projections = self.projections.read();
        projections.get(collection).cloned().or_else(|| {
            projections
                .iter()
                .find(|(stored, _)| name_matches(stored, collection))
                .map(|(_, metadata)| metadata.clone())
        })
    }

    #[must_use]
    pub fn list_projection_metadata(&self) -> Vec<ProjectionMeta> {
        let mut out = self
            .projections
            .read()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        out.sort_by_key(|projection| projection.collection.to_ascii_lowercase());
        out
    }

    #[must_use]
    pub fn get_materialized_projection(&self, name: &str) -> Option<ProjectionMeta> {
        let projections = self.projections.read();
        projections
            .get(name)
            .or_else(|| {
                projections
                    .iter()
                    .find(|(stored, metadata)| {
                        metadata.kind == ProjectionKind::Materialized && name_matches(stored, name)
                    })
                    .map(|(_, metadata)| metadata)
            })
            .filter(|metadata| metadata.kind == ProjectionKind::Materialized)
            .cloned()
    }

    #[must_use]
    pub fn is_materialized_projection(&self, name: &str) -> bool {
        self.get_materialized_projection(name).is_some()
    }

    #[must_use]
    pub fn materialized_projection_for_output(&self, output: &str) -> Option<ProjectionMeta> {
        self.projections
            .read()
            .values()
            .find(|projection| {
                projection.kind == ProjectionKind::Materialized
                    && projection
                        .versions
                        .iter()
                        .any(|version| version.output_collection == output)
            })
            .cloned()
    }

    pub fn unregister_projection_metadata(&self, collection: &str) {
        self.projections.write().remove(collection);
        self.bump_version();
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

    #[must_use]
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

    #[must_use]
    pub fn get_function(&self, name: &str) -> Option<FunctionMeta> {
        let key = name.to_ascii_lowercase();
        let functions = self.functions.read();
        functions.get(&key).cloned().or_else(|| {
            functions
                .values()
                .find(|function| name_matches(&function.name, name))
                .cloned()
        })
    }

    #[must_use]
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

    #[must_use]
    pub fn get_view(&self, name: &str) -> Option<ViewMeta> {
        let views = self.views.read();
        views.get(name).cloned().or_else(|| {
            views
                .iter()
                .find(|(stored, _)| name_matches(stored, name))
                .map(|(_, metadata)| metadata.clone())
        })
    }

    #[must_use]
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

    #[must_use]
    pub fn get_procedure(&self, name: &str) -> Option<ProcedureMeta> {
        let key = name.to_ascii_lowercase();
        let procedures = self.procedures.read();
        procedures.get(&key).cloned().or_else(|| {
            procedures
                .values()
                .find(|procedure| name_matches(&procedure.name, name))
                .cloned()
        })
    }

    #[must_use]
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

    #[must_use]
    pub fn get_role(&self, name: &str) -> Option<RoleMeta> {
        self.roles.read().get(&normalize_role_name(name)).cloned()
    }

    #[must_use]
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

    #[must_use]
    pub fn get_rollup(&self, name: &str) -> Option<RollupMeta> {
        let key = name.to_ascii_lowercase();
        let rollups = self.rollups.read();
        rollups.get(&key).cloned().or_else(|| {
            rollups
                .values()
                .find(|rollup| name_matches(&rollup.name, name))
                .cloned()
        })
    }

    #[must_use]
    pub fn list_rollups(&self) -> Vec<RollupMeta> {
        let mut out = self.rollups.read().values().cloned().collect::<Vec<_>>();
        out.sort_by_key(|rollup| rollup.name.to_ascii_lowercase());
        out
    }

    #[must_use]
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

    #[must_use]
    pub fn get_retention_policy(&self, name: &str) -> Option<RetentionPolicyMeta> {
        let key = name.to_ascii_lowercase();
        let policies = self.retention_policies.read();
        policies.get(&key).cloned().or_else(|| {
            policies
                .values()
                .find(|policy| name_matches(&policy.name, name))
                .cloned()
        })
    }

    #[must_use]
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
}
