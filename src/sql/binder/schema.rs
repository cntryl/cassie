use super::{
    bind_select, bm25, infer_select_schema, is_reserved_namespace, local_name,
    normalize_relation_name, normalize_schema_name, resolve_relation_name, resolve_schema_name,
    select_contains_parameters, virtual_views, AlterSchemaOperation, AlterSchemaStatement,
    AlterTableOperation, AlterTableStatement, BindingContext, CassieError, Catalog,
    CatalogObjectKind, CollectionSchema, CreateViewStatement, DataType, DistanceMetric,
    DropIndexStatement, DropSchemaStatement, DropViewStatement, Expr, HashMap, HashSet,
    QueryStatement,
};

#[path = "schema_alter_constraints.rs"]
mod schema_alter_constraints;
#[path = "schema_index_options.rs"]
mod schema_index_options;
#[path = "schema_indexes.rs"]
mod schema_indexes;
use super::schema_sequences::validate_alter_column_operation;
use schema_alter_constraints::validate_alter_constraint_targets;

pub(super) fn bind_create_table(
    mut statement: crate::sql::ast::CreateTableStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<crate::sql::ast::CreateTableStatement, CassieError> {
    let name = normalize_relation_name(statement.table.trim(), context)?;
    if name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE TABLE requires a table name".into(),
        ));
    }
    if !statement.if_not_exists
        && (catalog.relation_exists(&name) || virtual_views::schema(&name).is_some())
    {
        return Err(CassieError::Planner(format!(
            "collection '{name}' already exists"
        )));
    }

    if matches!(
        statement.storage_mode,
        crate::catalog::CollectionStorageMode::ColumnIndexed
    ) {
        return Err(CassieError::Planner(
            "CREATE TABLE storage mode 'column_indexed' is derived and cannot be created explicitly"
                .into(),
        ));
    }

    let mut seen = HashSet::new();
    let mut primary_key_field: Option<String> = None;
    for field in &mut statement.fields {
        let field_name = field.name.trim();
        if field_name.is_empty() {
            return Err(CassieError::Planner(
                "CREATE TABLE field names cannot be empty".into(),
            ));
        }

        if !seen.insert(field_name.to_ascii_lowercase()) {
            return Err(CassieError::Planner(format!(
                "CREATE TABLE field '{field_name}' is defined more than once"
            )));
        }

        for constraint in &mut field.constraints {
            if constraint.primary_key {
                if let Some(previous) = &primary_key_field {
                    return Err(CassieError::Planner(format!(
                        "multiple primary keys defined on '{name}': '{previous}' and '{field_name}'"
                    )));
                }
                primary_key_field = Some(field_name.to_string());
            }
            if let (Some(table), Some(reference_field)) = (
                constraint.references_table.as_deref(),
                constraint.references_field.as_deref(),
            ) {
                let table = resolve_relation_name(table, catalog, context)?;
                constraint.references_table = Some(table.clone());
                if !catalog.exists(&table) {
                    return Err(CassieError::CollectionNotFound(table.clone()));
                }
                let referenced_schema = catalog
                    .get_schema(&table)
                    .ok_or_else(|| CassieError::CollectionNotFound(table.clone()))?;
                if !referenced_schema
                    .fields
                    .iter()
                    .any(|entry| entry.name.eq_ignore_ascii_case(reference_field))
                {
                    return Err(CassieError::Planner(format!(
                        "foreign key on '{field_name}' references missing field '{reference_field}' on '{table}'"
                    )));
                }

                let references_supported =
                    catalog
                        .get_constraints(&table)
                        .into_iter()
                        .any(|candidate| {
                            candidate.field.eq_ignore_ascii_case(reference_field)
                                && (candidate.primary_key || candidate.unique)
                        })
                        || catalog
                            .list_indexes(&table)
                            .into_iter()
                            .filter(|index| {
                                index.unique && index.kind == crate::catalog::IndexKind::Scalar
                            })
                            .any(|index| {
                                let fields = index.normalized_fields();
                                fields.len() == 1 && fields[0].eq_ignore_ascii_case(reference_field)
                            });

                if !references_supported {
                    return Err(CassieError::Planner(format!(
                        "foreign key on '{field_name}' must reference a primary or unique key on '{table}.{reference_field}'"
                    )));
                }
            }
        }

        field.name = field_name.to_string();
    }

    statement.table = name;
    Ok(statement)
}

pub(super) fn bind_create_view(
    mut statement: CreateViewStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<CreateViewStatement, CassieError> {
    let name = normalize_relation_name(statement.name.trim(), context)?;
    if name.is_empty() {
        return Err(CassieError::Planner("CREATE VIEW requires a name".into()));
    }
    if !statement.if_not_exists
        && (catalog.relation_exists(&name) || virtual_views::schema(&name).is_some())
    {
        return Err(CassieError::Planner(format!(
            "relation '{name}' already exists"
        )));
    }

    let parsed = crate::sql::parser::parse_statement(&statement.query)
        .map_err(|error| CassieError::InvalidQuery(error.to_string()))?;
    let raw_sql = parsed.raw_sql.clone();
    let QueryStatement::Select(select) = parsed.statement else {
        return Err(CassieError::Planner(
            "CREATE VIEW requires a SELECT query body".into(),
        ));
    };

    let bound = bind_select(select, catalog, &HashMap::new(), context)?;
    if select_contains_parameters(&bound) {
        return Err(CassieError::Planner(
            "CREATE VIEW cannot contain bind parameters".into(),
        ));
    }

    let _schema = infer_select_schema(&bound, catalog)?;

    statement.name = name;
    statement.query = raw_sql;
    Ok(statement)
}

pub(super) fn bind_drop_view(
    mut statement: DropViewStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<DropViewStatement, CassieError> {
    let name = normalize_relation_name(statement.name.trim(), context)?;
    if name.is_empty() {
        return Err(CassieError::Planner("DROP VIEW requires a name".into()));
    }

    if catalog.get_view(&name).is_none() {
        if virtual_views::schema(&name).is_some() || catalog.exists(&name) {
            return Err(CassieError::Planner(format!(
                "relation '{name}' is not a view"
            )));
        }
        if !statement.if_exists {
            return Err(CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::View,
                name,
            });
        }
    }

    statement.name = name;
    Ok(statement)
}

pub(super) fn bind_create_graph(
    mut statement: crate::sql::ast::CreateGraphStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<crate::sql::ast::CreateGraphStatement, CassieError> {
    statement.name = normalize_relation_name(statement.name.trim(), context)?;
    if statement.name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE GRAPH requires a graph name".into(),
        ));
    }

    let node_table = format!("{}_nodes", statement.name);
    let edge_table = format!("{}_edges", statement.name);
    if !statement.if_not_exists
        && (catalog.graph_exists(&statement.name)
            || catalog.relation_exists(&node_table)
            || catalog.relation_exists(&edge_table)
            || virtual_views::schema(&node_table).is_some()
            || virtual_views::schema(&edge_table).is_some())
    {
        return Err(CassieError::Planner(format!(
            "graph '{}' already exists or its backing tables are unavailable",
            statement.name
        )));
    }

    validate_graph_fields("nodes", &statement.node_fields)?;
    validate_graph_fields("edges", &statement.edge_fields)?;
    Ok(statement)
}

pub(super) fn bind_create_index(
    statement: crate::sql::ast::CreateIndexStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<crate::sql::ast::CreateIndexStatement, CassieError> {
    schema_indexes::bind_create_index(statement, catalog, context)
}

fn validate_graph_fields(
    section: &str,
    fields: &[crate::sql::ast::FieldDefinition],
) -> Result<(), CassieError> {
    let mut seen = HashSet::new();
    for field in fields {
        let field_name = field.name.trim();
        if field_name.is_empty() {
            return Err(CassieError::Planner(format!(
                "CREATE GRAPH {section} field names cannot be empty"
            )));
        }
        if !seen.insert(field_name.to_ascii_lowercase()) {
            return Err(CassieError::Planner(format!(
                "CREATE GRAPH {section} field '{field_name}' is defined more than once"
            )));
        }
    }
    Ok(())
}

pub(super) fn bind_drop_index(
    mut statement: DropIndexStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<DropIndexStatement, CassieError> {
    let table = resolve_relation_name(statement.table.trim(), catalog, context)?;
    if table.is_empty() {
        return Err(CassieError::Planner(
            "DROP INDEX requires a collection name".into(),
        ));
    }
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner(
            "DROP INDEX requires an index name".into(),
        ));
    }

    if !catalog.exists(&table) {
        if !statement.if_exists {
            return Err(CassieError::CollectionNotFound(table));
        }
        statement.table = table;
        statement.name = name;
        return Ok(statement);
    }

    if !statement.if_exists && catalog.get_index(&table, &name).is_none() {
        return Err(CassieError::CatalogObjectNotFound {
            kind: CatalogObjectKind::Index,
            name,
        });
    }

    statement.table = table;
    statement.name = name;
    Ok(statement)
}

pub(super) fn bind_drop_schema(
    mut statement: DropSchemaStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<DropSchemaStatement, CassieError> {
    let schema = resolve_schema_name(statement.schema.trim(), catalog, context)?;
    if schema.is_empty() {
        return Err(CassieError::Planner(
            "DROP SCHEMA requires a schema name".into(),
        ));
    }
    if is_reserved_namespace(&local_name(&schema)) {
        return Err(CassieError::Unsupported(format!(
            "namespace '{schema}' is reserved"
        )));
    }

    statement.schema = schema;
    Ok(statement)
}

pub(super) fn bind_alter_schema(
    mut statement: AlterSchemaStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<AlterSchemaStatement, CassieError> {
    let schema = resolve_schema_name(statement.schema.trim(), catalog, context)?;
    if schema.is_empty() {
        return Err(CassieError::Planner(
            "ALTER SCHEMA requires a schema name".into(),
        ));
    }
    if is_reserved_namespace(&local_name(&schema)) {
        return Err(CassieError::Unsupported(format!(
            "namespace '{schema}' is reserved"
        )));
    }

    match &mut statement.operation {
        AlterSchemaOperation::RenameTo { schema: target } => {
            let next = normalize_schema_name(target.trim(), context)?;
            if next.is_empty() {
                return Err(CassieError::Planner(
                    "ALTER SCHEMA RENAME TO requires a schema name".into(),
                ));
            }
            if is_reserved_namespace(&local_name(&next)) {
                return Err(CassieError::Unsupported(format!(
                    "namespace '{next}' is reserved"
                )));
            }
            if schema.eq_ignore_ascii_case(&next) {
                return Err(CassieError::Planner(
                    "ALTER SCHEMA cannot rename namespace to same name".into(),
                ));
            }
            if catalog.namespace_exists(&next) {
                return Err(CassieError::Planner(format!(
                    "namespace '{next}' already exists"
                )));
            }
            *target = next;
        }
    }

    statement.schema = schema;
    Ok(statement)
}

pub(super) fn bind_drop_table(
    mut statement: crate::sql::ast::DropTableStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<crate::sql::ast::DropTableStatement, CassieError> {
    let table = resolve_relation_name(statement.table.trim(), catalog, context)?;
    if table.is_empty() {
        return Err(CassieError::Planner(
            "DROP TABLE requires a table name".into(),
        ));
    }
    if virtual_views::schema(&table).is_some() || catalog.get_view(&table).is_some() {
        return Err(CassieError::Planner(format!(
            "relation '{table}' is a view"
        )));
    }
    if !statement.if_exists && !catalog.exists(&table) {
        return Err(CassieError::CollectionNotFound(table));
    }
    statement.table = table;
    Ok(statement)
}

pub(super) fn bind_alter_table(
    mut statement: AlterTableStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<AlterTableStatement, CassieError> {
    let table = resolve_relation_name(statement.table.trim(), catalog, context)?;
    if table.is_empty() {
        return Err(CassieError::Planner(
            "ALTER TABLE requires a table name".into(),
        ));
    }
    if virtual_views::schema(&table).is_some() || catalog.get_view(&table).is_some() {
        return Err(CassieError::Planner(format!(
            "relation '{table}' is a view"
        )));
    }

    let schema = catalog
        .get_schema(&table)
        .ok_or_else(|| CassieError::CollectionNotFound(table.clone()))?;

    let existing_fields = schema
        .fields
        .into_iter()
        .map(|field| field.name.to_ascii_lowercase())
        .collect::<HashSet<_>>();

    validate_alter_schema(&table, &statement.operation, &existing_fields, catalog)?;
    validate_alter_constraint_targets(&statement.operation, catalog)?;

    statement.table = table;
    Ok(statement)
}

pub(super) fn validate_alter_schema(
    table: &str,
    operation: &AlterTableOperation,
    existing_fields: &HashSet<String>,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    match operation {
        AlterTableOperation::AddColumn {
            field,
            data_type: _,
        } => {
            validate_alter_add_column(table, field, existing_fields)?;
        }
        AlterTableOperation::AddConstraint { constraints } => {
            validate_alter_add_constraints(table, constraints, existing_fields)?;
        }
        AlterTableOperation::DropColumn { field } => {
            validate_alter_drop_column(table, field, existing_fields)?;
        }
        AlterTableOperation::RenameColumn { from, to } => {
            let from = from.trim();
            if from.is_empty() {
                return Err(CassieError::Planner(
                    "ALTER TABLE RENAME COLUMN requires a source field".into(),
                ));
            }
            let to = to.trim();
            if to.is_empty() {
                return Err(CassieError::Planner(
                    "ALTER TABLE RENAME COLUMN requires a target field".into(),
                ));
            }
            if from.eq_ignore_ascii_case(to) {
                return Err(CassieError::Planner(
                    "ALTER TABLE cannot rename column to same name".into(),
                ));
            }
            if !existing_fields.contains(&from.to_ascii_lowercase()) {
                return Err(CassieError::Planner(format!(
                    "ALTER TABLE '{table}' has no field '{from}'"
                )));
            }
            if existing_fields.contains(&to.to_ascii_lowercase()) {
                return Err(CassieError::Planner(format!(
                    "cannot rename column to existing field '{to}' on collection '{table}'"
                )));
            }
        }
        AlterTableOperation::RenameTo { table: target } => {
            let target = target.trim();
            if target.is_empty() {
                return Err(CassieError::Planner(
                    "ALTER TABLE RENAME TO requires a table name".into(),
                ));
            }
            if table.eq_ignore_ascii_case(target) {
                return Err(CassieError::Planner(
                    "ALTER TABLE cannot rename collection to same name".into(),
                ));
            }
        }
        AlterTableOperation::AlterColumnSetDefault { .. }
        | AlterTableOperation::AlterColumnDropDefault { .. }
        | AlterTableOperation::AlterColumnSetNotNull { .. }
        | AlterTableOperation::AlterColumnDropNotNull { .. } => {
            validate_alter_column_operation(table, operation, existing_fields, catalog)?;
        }
    }

    Ok(())
}

fn validate_alter_add_column(
    table: &str,
    field: &str,
    existing_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    let name = field.trim();
    if name.is_empty() {
        return Err(CassieError::Planner(
            "ALTER TABLE ADD COLUMN requires a field name".into(),
        ));
    }
    if existing_fields.contains(&name.to_ascii_lowercase()) {
        return Err(CassieError::Planner(format!(
            "cannot add existing column '{name}' on collection '{table}'"
        )));
    }
    Ok(())
}

fn validate_alter_add_constraints(
    table: &str,
    constraints: &[crate::catalog::FieldConstraint],
    existing_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    if constraints.is_empty() {
        return Err(CassieError::Planner(
            "ALTER TABLE ADD CONSTRAINT requires a constraint".into(),
        ));
    }
    for constraint in constraints {
        let name = constraint.field.trim();
        if name.is_empty() {
            return Err(CassieError::Planner(
                "ALTER TABLE ADD CONSTRAINT requires a field".into(),
            ));
        }
        if !existing_fields.contains(&name.to_ascii_lowercase()) {
            return Err(CassieError::Planner(format!(
                "ALTER TABLE '{table}' has no field '{name}'"
            )));
        }
    }
    Ok(())
}

fn validate_alter_drop_column(
    table: &str,
    field: &str,
    existing_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    let name = field.trim();
    if name.is_empty() {
        return Err(CassieError::Planner(
            "ALTER TABLE DROP COLUMN requires a field name".into(),
        ));
    }
    if name.eq_ignore_ascii_case("id") {
        return Err(CassieError::Planner(
            "ALTER TABLE DROP COLUMN cannot remove reserved field 'id'".into(),
        ));
    }
    if !existing_fields.contains(&name.to_ascii_lowercase()) {
        return Err(CassieError::Planner(format!(
            "ALTER TABLE '{table}' has no field '{name}'"
        )));
    }
    Ok(())
}
