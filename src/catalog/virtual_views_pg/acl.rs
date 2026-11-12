use crate::catalog::{
    canonical_schema_name, Catalog, RoleGrantTarget, RoleObjectGrantMeta, RoleObjectPrivilege,
    SchemaId,
};
use crate::types::Value;
use std::collections::HashMap;

pub(super) fn schema_acl(catalog: &Catalog, current_database: Option<&str>, schema: &str) -> Value {
    let Some(database_name) = current_database else {
        return Value::Null;
    };
    let Some(database) = catalog.get_database(database_name) else {
        return Value::Null;
    };
    let canonical = SchemaId::parse_canonical(schema).map_or_else(
        || canonical_schema_name(database_name, schema),
        |_| schema.to_owned(),
    );
    let Some(namespace) = catalog.get_namespace(&canonical) else {
        return Value::Null;
    };
    acl_value(
        catalog,
        &database.physical_family,
        &RoleGrantTarget::Schema(namespace.object_id),
        true,
    )
}

pub(super) fn table_acl(
    catalog: &Catalog,
    current_database: Option<&str>,
    relation: &str,
    kind: &str,
) -> Value {
    if kind != "r" {
        return Value::Null;
    }
    let Some(database_name) = current_database else {
        return Value::Null;
    };
    let Some(database) = catalog.get_database(database_name) else {
        return Value::Null;
    };
    let Some(table) = catalog.get_collection(relation) else {
        return Value::Null;
    };
    acl_value(
        catalog,
        &database.physical_family,
        &RoleGrantTarget::Table(table.storage_id),
        false,
    )
}

fn acl_value(
    catalog: &Catalog,
    database_family: &str,
    target: &RoleGrantTarget,
    is_schema: bool,
) -> Value {
    let role_names = catalog
        .list_roles()
        .into_iter()
        .map(|role| (role.object_id, role.name))
        .collect::<HashMap<_, _>>();
    let mut entries = catalog
        .list_role_object_grants()
        .into_iter()
        .filter(|grant| grant.database_family == database_family && &grant.target == target)
        .filter_map(|grant| {
            let grantee = role_names.get(&grant.role_id)?;
            let privileges = selected_privilege_text(&grant, is_schema);
            if privileges.is_empty() {
                return None;
            }
            Some((
                grantee.clone(),
                crate::catalog::object_identity::quote_identifier(grantee),
                privileges,
            ))
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    if entries.is_empty() {
        Value::Null
    } else {
        Value::String(format!(
            "{{{}}}",
            entries
                .into_iter()
                .map(|(_, grantee, privileges)| format!("{grantee}={privileges}/root"))
                .collect::<Vec<_>>()
                .join(",")
        ))
    }
}

fn selected_privilege_text(grant: &RoleObjectGrantMeta, is_schema: bool) -> String {
    if is_schema {
        return if grant.grants(RoleObjectPrivilege::Usage) {
            "U".to_owned()
        } else {
            String::new()
        };
    }
    [
        (RoleObjectPrivilege::Insert, 'a'),
        (RoleObjectPrivilege::Select, 'r'),
        (RoleObjectPrivilege::Update, 'w'),
        (RoleObjectPrivilege::Delete, 'd'),
    ]
    .into_iter()
    .filter_map(|(privilege, letter)| grant.grants(privilege).then_some(letter))
    .collect()
}
