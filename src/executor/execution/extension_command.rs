use super::command_output::empty as empty_command;
use super::{Cassie, CassieSession, QueryError, QueryResult};
use crate::app::{CassieError, DiagnosticError};
use crate::catalog::{
    canonical_schema_name, relation_belongs_to_database, ExtensionMeta, IndexKind,
    VectorAnnCatalogIdentityIds, VectorCatalogIdentitySet, VectorDistanceOperatorIds, ViewMeta,
    DEFAULT_SCHEMA,
};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{ExtensionAction, ExtensionStatement};

const PGCRYPTO_VERSION: &str = "1.4";
const VECTOR_VERSION: &str = "0.8.7";
const ROOT_OID: u32 = 10;

#[derive(Clone, Copy)]
struct CreateExtension<'a> {
    name: &'a str,
    if_not_exists: bool,
    default_schema: &'a str,
    requested_schema: Option<&'a str>,
    requested_version: Option<&'a str>,
}

pub(super) fn execute(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &ExtensionStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    if session.is_some_and(|session| !session.user.eq_ignore_ascii_case("root")) {
        return Err(CassieError::from(DiagnosticError::new(
            "42501",
            "must be root to manage installed extensions",
        ))
        .into());
    }
    let database = session
        .and_then(CassieSession::current_database)
        .unwrap_or(&cassie.default_database)
        .to_owned();
    match &statement.action {
        ExtensionAction::Create {
            name,
            if_not_exists,
            schema,
            version,
        } => {
            let default_schema = session.map_or_else(
                || DEFAULT_SCHEMA.to_owned(),
                |session| {
                    cassie
                        .binding_context_for_session(Some(session))
                        .current_schema()
                        .to_owned()
                },
            );
            create(
                cassie,
                &database,
                CreateExtension {
                    name,
                    if_not_exists: *if_not_exists,
                    default_schema: &default_schema,
                    requested_schema: schema.as_deref(),
                    requested_version: version.as_deref(),
                },
                controls,
            )
        }
        ExtensionAction::SetSchema { name, schema } => {
            set_schema(cassie, &database, name, schema, controls)
        }
        ExtensionAction::Update { name, version } => {
            update(cassie, &database, name, version.as_deref(), controls)
        }
        ExtensionAction::Drop {
            name,
            if_exists,
            cascade,
        } => drop_extension(cassie, &database, name, *if_exists, *cascade, controls),
    }
}

fn create(
    cassie: &Cassie,
    database: &str,
    request: CreateExtension<'_>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let CreateExtension {
        name,
        if_not_exists,
        default_schema,
        requested_schema,
        requested_version,
    } = request;
    let name = name.to_ascii_lowercase();
    if cassie.catalog.get_extension(database, &name).is_some() {
        if if_not_exists {
            return empty_command("CREATE EXTENSION", controls);
        }
        return Err(CassieError::from(DiagnosticError::new(
            "42710",
            format!("extension \"{name}\" already exists"),
        ))
        .into());
    }
    let selected_version = selected_version(&name, requested_version)?;
    let schema = requested_schema.unwrap_or(default_schema);
    let canonical_schema = canonical_schema_name(database, schema);
    if !schema_exists(cassie, &canonical_schema, schema) {
        return Err(CassieError::from(DiagnosticError::new(
            "3F000",
            format!("schema \"{schema}\" does not exist"),
        ))
        .into());
    }
    let output = empty_command("CREATE EXTENSION", controls)?;
    let metadata = ExtensionMeta {
        format_version: 2,
        database: database.to_owned(),
        name: name.clone(),
        version: selected_version.to_owned(),
        schema: canonical_schema,
        owner_oid: ROOT_OID,
        object_id: cassie
            .midge
            .allocate_object_id()
            .map_err(QueryError::from)?,
        digest_text_object_id: if name == "pgcrypto" {
            Some(
                cassie
                    .midge
                    .allocate_object_id()
                    .map_err(QueryError::from)?,
            )
        } else {
            None
        },
        digest_bytea_object_id: if name == "pgcrypto" {
            Some(
                cassie
                    .midge
                    .allocate_object_id()
                    .map_err(QueryError::from)?,
            )
        } else {
            None
        },
        vector_type_object_id: if name == "vector" {
            Some(
                cassie
                    .midge
                    .allocate_object_id()
                    .map_err(QueryError::from)?,
            )
        } else {
            None
        },
        vector_catalog_identities: if name == "vector" {
            Some(allocate_vector_catalog_identities(cassie)?)
        } else {
            None
        },
    };
    cassie
        .midge
        .put_extension(&metadata)
        .map_err(QueryError::from)?;
    cassie.catalog.register_extension(metadata);
    Ok(output)
}

fn allocate_vector_catalog_identities(
    cassie: &Cassie,
) -> Result<VectorCatalogIdentitySet, QueryError> {
    Ok(VectorCatalogIdentitySet {
        operators: VectorDistanceOperatorIds {
            l2: allocate_catalog_object_id(cassie)?,
            inner_product: allocate_catalog_object_id(cassie)?,
            cosine: allocate_catalog_object_id(cassie)?,
        },
        hnsw: allocate_vector_ann_catalog_identities(cassie)?,
        ivfflat: allocate_vector_ann_catalog_identities(cassie)?,
    })
}

fn allocate_vector_ann_catalog_identities(
    cassie: &Cassie,
) -> Result<VectorAnnCatalogIdentityIds, QueryError> {
    Ok(VectorAnnCatalogIdentityIds {
        access_method: allocate_catalog_object_id(cassie)?,
        l2_operator_family: allocate_catalog_object_id(cassie)?,
        inner_product_operator_family: allocate_catalog_object_id(cassie)?,
        cosine_operator_family: allocate_catalog_object_id(cassie)?,
        l2_operator_class: allocate_catalog_object_id(cassie)?,
        inner_product_operator_class: allocate_catalog_object_id(cassie)?,
        cosine_operator_class: allocate_catalog_object_id(cassie)?,
    })
}

fn allocate_catalog_object_id(cassie: &Cassie) -> Result<u64, QueryError> {
    cassie
        .midge
        .allocate_object_id()
        .map_err(QueryError::from)
}

fn set_schema(
    cassie: &Cassie,
    database: &str,
    name: &str,
    schema: &str,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let name = name.to_ascii_lowercase();
    let Some(mut metadata) = cassie.catalog.get_extension(database, &name) else {
        return Err(undefined_extension(&name));
    };
    let canonical_schema = canonical_schema_name(database, schema);
    if !schema_exists(cassie, &canonical_schema, schema) {
        return Err(CassieError::from(DiagnosticError::new(
            "3F000",
            format!("schema \"{schema}\" does not exist"),
        ))
        .into());
    }
    let output = empty_command("ALTER EXTENSION", controls)?;
    metadata.schema = canonical_schema;
    cassie
        .midge
        .put_extension(&metadata)
        .map_err(QueryError::from)?;
    cassie.catalog.register_extension(metadata);
    Ok(output)
}

fn update(
    cassie: &Cassie,
    database: &str,
    name: &str,
    requested_version: Option<&str>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let name = name.to_ascii_lowercase();
    let Some(metadata) = cassie.catalog.get_extension(database, &name) else {
        return Err(undefined_extension(&name));
    };
    if requested_version.is_some_and(|version| !version.eq_ignore_ascii_case(&metadata.version)) {
        return Err(CassieError::Unsupported(format!(
            "extension {name} has no selected update target {}",
            requested_version.unwrap_or_default()
        ))
        .into());
    }
    empty_command("ALTER EXTENSION", controls)
}

fn drop_extension(
    cassie: &Cassie,
    database: &str,
    name: &str,
    if_exists: bool,
    cascade: bool,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let name = name.to_ascii_lowercase();
    let Some(metadata) = cassie.catalog.get_extension(database, &name) else {
        return if if_exists {
            empty_command("DROP EXTENSION", controls)
        } else {
            Err(undefined_extension(&name))
        };
    };
    let dependent_views = views_depending_on_extension(cassie, database, &metadata);
    let vector_dependents = if name == "vector" {
        vector_extension_dependents(cassie, database)
    } else {
        Vec::new()
    };
    if name == "vector" && cascade && !vector_dependents.is_empty() {
        return Err(CassieError::Unsupported(format!(
            "DROP EXTENSION vector CASCADE with dependent vector columns, materialized views, or ANN indexes is not supported: {}",
            vector_dependents.join(", ")
        ))
        .into());
    }
    if !cascade && (!dependent_views.is_empty() || !vector_dependents.is_empty()) {
        let dependent = dependent_views
            .iter()
            .map(|view| view.name.as_str())
            .chain(vector_dependents.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(CassieError::from(DiagnosticError::new(
            "2BP01",
            format!(
                "cannot drop extension \"{name}\" because other objects depend on it: {dependent}"
            ),
        ))
        .into());
    }
    let output = empty_command("DROP EXTENSION", controls)?;
    let view_names = dependent_views
        .iter()
        .map(|view| view.name.clone())
        .collect::<Vec<_>>();
    cassie
        .midge
        .delete_extension_and_views(database, &name, &view_names)
        .map_err(QueryError::from)?;
    cassie.catalog.unregister_extension(database, &name);
    for view in dependent_views {
        cassie.catalog.unregister_view(&view.name);
    }
    Ok(output)
}

fn selected_version(name: &str, requested: Option<&str>) -> Result<&'static str, QueryError> {
    let selected = match name {
        "pgcrypto" => PGCRYPTO_VERSION,
        "vector" => VECTOR_VERSION,
        _ => {
            return Err(CassieError::Unsupported(format!(
                "extension {name} is outside the selected built-in profiles"
            ))
            .into());
        }
    };
    if requested.is_some_and(|version| !version.eq_ignore_ascii_case(selected)) {
        return Err(CassieError::Unsupported(format!(
            "extension {name} version {} is outside the selected built-in profile {selected}",
            requested.unwrap_or_default()
        ))
        .into());
    }
    Ok(selected)
}

fn schema_exists(cassie: &Cassie, canonical: &str, schema: &str) -> bool {
    schema.eq_ignore_ascii_case(DEFAULT_SCHEMA) || cassie.catalog.namespace_exists(canonical)
}

fn undefined_extension(name: &str) -> QueryError {
    CassieError::from(DiagnosticError::new(
        "42704",
        format!("extension \"{name}\" does not exist"),
    ))
    .into()
}

fn views_depending_on_extension(
    cassie: &Cassie,
    database: &str,
    extension: &ExtensionMeta,
) -> Vec<ViewMeta> {
    cassie
        .catalog
        .list_views()
        .into_iter()
        .filter(|view| crate::catalog::relation_belongs_to_database(&view.name, database))
        .filter(|view| {
            view.extension_dependency_ids.contains(&extension.object_id)
                || (extension.name.eq_ignore_ascii_case("vector")
                    && view
                        .schema
                        .fields
                        .iter()
                        .any(|field| crate::sql::binder::data_type_uses_vector(&field.data_type)))
        })
        .collect()
}

fn vector_extension_dependents(cassie: &Cassie, database: &str) -> Vec<String> {
    let mut dependents = Vec::new();
    for collection in cassie.catalog.list_collections_canonical() {
        if !relation_belongs_to_database(&collection.name, database) {
            continue;
        }
        if let Some(schema) = cassie.catalog.get_schema(&collection.name) {
            let relation_name = cassie
                .catalog
                .materialized_projection_for_output(&collection.name)
                .filter(|projection| {
                    projection.materialized.as_ref().is_some_and(
                        crate::catalog::MaterializedProjectionMeta::is_sql_materialized_view,
                    )
                })
                .map_or_else(|| collection.name.clone(), |projection| projection.collection);
            dependents.extend(
                schema
                    .fields
                    .iter()
                    .filter(|field| crate::sql::binder::data_type_uses_vector(&field.data_type))
                    .map(|field| format!("{relation_name}.{}", field.name)),
            );
        }
    }
    dependents.extend(
        cassie
            .catalog
            .all_indexes_snapshot()
            .into_iter()
            .filter(|index| {
                index.kind == IndexKind::Vector
                    && relation_belongs_to_database(&index.collection, database)
            })
            .map(|index| index.name),
    );
    dependents.sort_unstable();
    dependents.dedup();
    dependents
}
