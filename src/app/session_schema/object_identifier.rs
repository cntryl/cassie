use super::CassieSession;
use crate::catalog::{
    canonical_schema_name, is_system_schema, Catalog, RoleGrantTarget, RoleObjectPrivilege,
};
use crate::sql::binder::BindingContext;

impl CassieSession {
    /// The catalog and search path used by `reg*` name input.
    ///
    /// Network-authenticated non-admin roles search only schemas where they
    /// have direct USAGE. Qualified names are checked separately by
    /// [`Self::object_identifier_unusable_schemas`].
    pub(crate) fn object_identifier_lookup_context(&self) -> Option<(Catalog, BindingContext)> {
        let (catalog, mut context) = self.object_lookup_context()?;
        if !self.is_network_authenticated() || !self.is_authenticated_read_only() {
            return Some((catalog, context));
        }

        let (role_id, database, database_family) = self.usage_subject(&catalog, &context);
        context.search_path.retain(|schema| {
            usable(
                &catalog,
                role_id,
                database_family.as_deref(),
                &database,
                schema,
            )
        });
        Some((catalog, context))
    }

    fn usage_subject(
        &self,
        catalog: &Catalog,
        context: &BindingContext,
    ) -> (Option<u64>, String, Option<String>) {
        let database = context.database.clone();
        let family = catalog
            .get_database(&database)
            .map(|database| database.physical_family);
        (
            catalog.get_role(&self.user).map(|role| role.object_id),
            database,
            family,
        )
    }

    /// Lowercase names of the schemas qualified `reg*` names may not use: for a
    /// network-authenticated non-admin role, every stored schema (and
    /// `public`) without direct `USAGE`. Other sessions are unrestricted.
    pub(crate) fn object_identifier_unusable_schemas(&self) -> std::collections::HashSet<String> {
        let Some((catalog, context)) = self.object_lookup_context() else {
            return std::collections::HashSet::new();
        };
        if !self.is_network_authenticated() || !self.is_authenticated_read_only() {
            return std::collections::HashSet::new();
        }
        let (role_id, database, family) = self.usage_subject(&catalog, &context);
        let prefix = format!("{database}.");
        catalog
            .list_namespaces()
            .into_iter()
            .filter_map(|namespace| namespace.name.strip_prefix(&prefix).map(str::to_owned))
            .chain(std::iter::once("public".to_owned()))
            .filter(|schema| !usable(&catalog, role_id, family.as_deref(), &database, schema))
            .map(|schema| schema.trim_matches('"').to_ascii_lowercase())
            .collect()
    }
}

fn usable(
    catalog: &Catalog,
    role_id: Option<u64>,
    database_family: Option<&str>,
    database: &str,
    schema: &str,
) -> bool {
    if is_system_schema(schema) {
        return true;
    }
    let (Some(role_id), Some(database_family)) = (role_id, database_family) else {
        return false;
    };
    let canonical = canonical_schema_name(database, schema);
    let Some(namespace) = catalog.get_namespace(&canonical) else {
        return false;
    };
    catalog
        .get_role_object_grant(
            database_family,
            role_id,
            &RoleGrantTarget::Schema(namespace.object_id),
        )
        .is_some_and(|grant| grant.grants(RoleObjectPrivilege::Usage))
}
