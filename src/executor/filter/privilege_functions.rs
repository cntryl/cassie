use super::{text_arg, CassieSession, QueryError, Value};
use crate::app::{CassieError, CatalogObjectKind};
use crate::catalog::object_identity::ObjectScope;
use crate::catalog::{normalize_role_name, Catalog, RoleGrantTarget, RoleMeta, RoleObjectPrivilege};
use crate::sql::binder::BindingContext;
use crate::types::object_id::FIRST_NORMAL_OBJECT_ID;
use crate::types::ObjectIdKind;

#[derive(Clone, Copy)]
enum ObjectKind {
    Database,
    Schema,
    Table,
}

#[derive(Clone, Copy)]
enum RequestedPrivilege {
    Connect,
    Object(RoleObjectPrivilege),
}

pub(super) fn evaluate(
    name: &str,
    args: &[Value],
    session: Option<&CassieSession>,
) -> Result<Value, QueryError> {
    super::require_arg_count_range(name, args, 2..=3)?;
    if args.iter().any(|value| matches!(value, Value::Null)) {
        return Ok(Value::Null);
    }

    let kind = object_kind(name);
    let inputs = args
        .iter()
        .map(|value| {
            text_arg(name, value)?.ok_or_else(|| {
                QueryError::General(format!("function '{name}' received a null input"))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let privilege_input = inputs.last().ok_or_else(|| {
        QueryError::General(format!("function '{name}' requires a privilege input"))
    })?;
    let requested = parse_privileges(name, kind, privilege_input)?;
    if inputs.len() == 3 {
        reject_oid_argument(name, "role", &inputs[0])?;
    }
    let session = session
        .ok_or_else(|| QueryError::General(format!("function '{name}' requires a session")))?;
    let (catalog, context) = session.object_identifier_lookup_context().ok_or_else(|| {
        QueryError::General(format!("function '{name}' requires a session catalog"))
    })?;
    let role = if inputs.len() == 3 {
        explicit_role(&catalog, &inputs[0])?
    } else {
        current_role(&catalog, session)?
    };
    let object_name = &inputs[if inputs.len() == 3 { 1 } else { 0 }];
    reject_oid_argument(name, "object", object_name)?;

    let allowed = match kind {
        ObjectKind::Database => {
            database_privilege(&catalog, &role, object_name, &requested)?
        }
        ObjectKind::Schema => {
            schema_privilege(&catalog, &context, &role, object_name, &requested)?
        }
        ObjectKind::Table => {
            table_privilege(&catalog, &context, &role, object_name, &requested)?
        }
    };
    Ok(Value::Bool(allowed))
}

fn object_kind(name: &str) -> ObjectKind {
    match name.strip_prefix("pg_catalog.").unwrap_or(name) {
        "has_database_privilege" => ObjectKind::Database,
        "has_schema_privilege" => ObjectKind::Schema,
        _ => ObjectKind::Table,
    }
}

fn reject_oid_argument(name: &str, argument: &str, input: &str) -> Result<(), QueryError> {
    let trimmed = input.trim();
    if trimmed == "-"
        || trimmed
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
    {
        return Err(QueryError::Cassie(CassieError::Unsupported(format!(
            "{name} does not support {argument} OID arguments"
        ))));
    }
    Ok(())
}

fn parse_privileges(
    name: &str,
    kind: ObjectKind,
    input: &str,
) -> Result<Vec<RequestedPrivilege>, QueryError> {
    input
        .split(',')
        .map(str::trim)
        .map(|privilege| {
            let selected = match (kind, privilege.to_ascii_uppercase().as_str()) {
                (ObjectKind::Database, "CONNECT") => Some(RequestedPrivilege::Connect),
                (ObjectKind::Schema, "USAGE") => {
                    Some(RequestedPrivilege::Object(RoleObjectPrivilege::Usage))
                }
                (ObjectKind::Table, "SELECT") => {
                    Some(RequestedPrivilege::Object(RoleObjectPrivilege::Select))
                }
                (ObjectKind::Table, "INSERT") => {
                    Some(RequestedPrivilege::Object(RoleObjectPrivilege::Insert))
                }
                (ObjectKind::Table, "UPDATE") => {
                    Some(RequestedPrivilege::Object(RoleObjectPrivilege::Update))
                }
                (ObjectKind::Table, "DELETE") => {
                    Some(RequestedPrivilege::Object(RoleObjectPrivilege::Delete))
                }
                _ => None,
            };
            selected.ok_or_else(|| {
                QueryError::Cassie(CassieError::Unsupported(format!(
                    "{name} does not support privilege '{privilege}'"
                )))
            })
        })
        .collect()
}

fn explicit_role(catalog: &Catalog, name: &str) -> Result<RoleMeta, QueryError> {
    let normalized = normalize_role_name(name);
    catalog
        .get_role(&normalized)
        .ok_or_else(|| {
            QueryError::Cassie(CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::Role,
                name: normalized,
            })
        })
}

fn current_role(catalog: &Catalog, session: &CassieSession) -> Result<RoleMeta, QueryError> {
    if !session.is_network_authenticated() {
        return Ok(RoleMeta::bootstrap_admin("root", None));
    }
    catalog.get_role(&session.user).or_else(|| {
        session.user.eq_ignore_ascii_case("root")
            .then(|| RoleMeta::bootstrap_admin("root", None))
    })
    .ok_or_else(|| {
        QueryError::Cassie(CassieError::CatalogObjectNotFound {
            kind: CatalogObjectKind::Role,
            name: session.user.clone(),
        })
    })
}

fn database_privilege(
    catalog: &Catalog,
    role: &RoleMeta,
    name: &str,
    requested: &[RequestedPrivilege],
) -> Result<bool, QueryError> {
    let database = catalog
        .list_databases()
        .into_iter()
        .find(|database| database.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| {
            QueryError::Cassie(CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::Database,
                name: name.to_owned(),
            })
        })?;
    Ok(requested
        .iter()
        .any(|privilege| matches!(*privilege, RequestedPrivilege::Connect))
        && role.can_access_database(&database.name))
}

fn schema_privilege(
    catalog: &Catalog,
    context: &BindingContext,
    role: &RoleMeta,
    name: &str,
    requested: &[RequestedPrivilege],
) -> Result<bool, QueryError> {
    let scope = ObjectScope::new(catalog, context);
    let oid = resolve_object_oid(&scope, ObjectIdKind::RegNamespace, name)?;
    let system_schema = crate::catalog::is_system_schema(
        &scope.label(ObjectIdKind::RegNamespace, oid),
    );
    if role.is_admin || system_schema {
        return Ok(true);
    }
    let Some(namespace) = catalog.list_namespaces().into_iter().find(|namespace| {
        crate::catalog::object_identity::stored_namespace_oid(catalog, &namespace.name)
            == Some(oid)
    }) else {
        return Ok(false);
    };
    let Some(database) = catalog.get_database(&context.database) else {
        return Ok(false);
    };
    let grant = catalog.get_role_object_grant(
        &database.physical_family,
        role.object_id,
        &RoleGrantTarget::Schema(namespace.object_id),
    );
    Ok(requested
        .iter()
        .any(|privilege| requested_object_grant(grant.as_ref(), privilege)))
}

fn table_privilege(
    catalog: &Catalog,
    context: &BindingContext,
    role: &RoleMeta,
    name: &str,
    requested: &[RequestedPrivilege],
) -> Result<bool, QueryError> {
    let scope = ObjectScope::new(catalog, context);
    let oid = resolve_object_oid(&scope, ObjectIdKind::RegClass, name)?;
    if role.is_admin || oid < FIRST_NORMAL_OBJECT_ID {
        return Ok(true);
    }
    let Some(table) = catalog.list_collections_canonical().into_iter().find(|table| {
        crate::catalog::object_identity::stored_relation_oid(catalog, &table.name) == Some(oid)
    }) else {
        return Ok(false);
    };
    let Some(database) = catalog.get_database(&context.database) else {
        return Ok(false);
    };
    let grant = catalog.get_role_object_grant(
        &database.physical_family,
        role.object_id,
        &RoleGrantTarget::Table(table.storage_id),
    );
    Ok(requested
        .iter()
        .any(|privilege| requested_object_grant(grant.as_ref(), privilege)))
}

fn requested_object_grant(
    grant: Option<&crate::catalog::RoleObjectGrantMeta>,
    requested: &RequestedPrivilege,
) -> bool {
    match (grant, requested) {
        (Some(grant), RequestedPrivilege::Object(privilege)) => grant.grants(*privilege),
        _ => false,
    }
}

fn resolve_object_oid(
    scope: &ObjectScope<'_>,
    kind: ObjectIdKind,
    name: &str,
) -> Result<u32, QueryError> {
    let oid = scope
        .resolve_required(kind, name)
        .map_err(|error| QueryError::Cassie(error.into()))?;
    let label = scope.label(kind, oid);
    if oid == 0 || label == oid.to_string() {
        return Err(QueryError::Cassie(
            crate::types::ObjectIdentifierError::Undefined {
                kind,
                name: name.trim().to_owned(),
            }
            .into(),
        ));
    }
    Ok(oid)
}
