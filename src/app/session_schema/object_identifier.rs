use super::CassieSession;
use crate::catalog::{
    canonical_schema_name, is_system_schema, Catalog, RoleGrantTarget, RoleObjectPrivilege,
};
use crate::sql::binder::BindingContext;

impl CassieSession {
    /// The catalog and search path used by `reg*` name input.
    ///
    /// Network-authenticated non-admin roles search only schemas where they
    /// have direct USAGE. Explicitly qualified object and namespace lookups
    /// remain independent of this search path.
    pub(crate) fn object_identifier_lookup_context(&self) -> Option<(Catalog, BindingContext)> {
        let (catalog, mut context) = self.object_lookup_context()?;
        if !self.is_network_authenticated() || !self.is_authenticated_read_only() {
            return Some((catalog, context));
        }

        let role_id = catalog.get_role(&self.user).map(|role| role.object_id);
        let database = context.database.clone();
        let database_family = catalog
            .get_database(&database)
            .map(|database| database.physical_family);
        context.search_path.retain(|schema| {
            if is_system_schema(schema) {
                return true;
            }
            let (Some(role_id), Some(database_family)) =
                (role_id.as_ref(), database_family.as_deref())
            else {
                return false;
            };
            let canonical = canonical_schema_name(&database, schema);
            let Some(namespace) = catalog.get_namespace(&canonical) else {
                return false;
            };
            catalog
                .get_role_object_grant(
                    database_family,
                    *role_id,
                    &RoleGrantTarget::Schema(namespace.object_id),
                )
                .is_some_and(|grant| grant.grants(RoleObjectPrivilege::Usage))
        });
        Some((catalog, context))
    }
}
