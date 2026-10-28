use super::{
    bind_select, bm25, infer_select_schema_with_context, is_reserved_namespace, local_name,
    normalize_relation_name, normalize_schema_name, resolve_relation_name, resolve_relation_path,
    resolve_schema_name, select_contains_parameters, virtual_views, AlterIndexOperation,
    AlterIndexStatement, AlterSchemaOperation, AlterSchemaStatement, AlterTableOperation,
    AlterTableStatement, BindingContext, CassieError, Catalog, CatalogObjectKind, CollectionSchema,
    CreateViewStatement, DistanceMetric, DropIndexStatement, DropSchemaStatement,
    DropViewStatement, Expr, HashSet, QueryStatement, ReindexStatement, ReindexTarget,
};

#[path = "schema_alter_constraints.rs"]
mod schema_alter_constraints;
#[path = "schema_checks.rs"]
mod schema_checks;
#[path = "schema_defaults.rs"]
mod schema_defaults;
#[path = "schema_index_options.rs"]
mod schema_index_options;
#[path = "schema_indexes.rs"]
mod schema_indexes;
use super::schema_sequences::validate_alter_column_operation;
use crate::catalog::{canonical_relation_name, parse_name, ParsedName};
use crate::sql::ast::IdentifierPath;
use schema_alter_constraints::bind_alter_constraint_targets;
#[path = "schema_create_table.rs"]
mod schema_create_table;
pub(super) use schema_create_table::{
    bind_create_table, canonical_relation_path, resolve_default_sequence,
};

pub(super) fn bind_create_view(
    mut statement: CreateViewStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<CreateViewStatement, CassieError> {
    let name = super::normalize_new_relation_name(statement.name.trim(), context, catalog)?;
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
    crate::sql::definition_guard::statement(&parsed)?;
    // Persist the whole body text: a set operation's or WITH query's parsed
    // `raw_sql` covers only its leading SELECT.
    let body = statement
        .query
        .trim()
        .trim_end_matches(';')
        .trim()
        .to_string();
    let QueryStatement::Select(select) = parsed.statement else {
        return Err(CassieError::Planner(
            "CREATE VIEW requires a SELECT query body".into(),
        ));
    };

    let bound = bind_select(*select, catalog, &super::CteScope::default(), context)?;
    if select_contains_parameters(&bound) {
        return Err(CassieError::Planner(
            "CREATE VIEW cannot contain bind parameters".into(),
        ));
    }

    let _schema = infer_select_schema_with_context(&bound, catalog, context)?;

    statement.name = name;
    statement.query = body;
    Ok(statement)
}

pub(super) fn bind_drop_view(
    mut statement: DropViewStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<DropViewStatement, CassieError> {
    let names = std::iter::once(statement.name)
        .chain(statement.additional_names)
        .map(|name| bind_drop_view_name(&name, statement.if_exists, catalog, context))
        .collect::<Result<Vec<_>, _>>()?;
    let mut names = names.into_iter();
    statement.name = names
        .next()
        .ok_or_else(|| CassieError::Planner("DROP VIEW requires a name".into()))?;
    statement.additional_names = names.collect();
    Ok(statement)
}

fn bind_drop_view_name(
    raw_name: &str,
    if_exists: bool,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<String, CassieError> {
    let name = super::resolve_existing_name(raw_name.trim(), context, |name| {
        catalog.relation_exists(name) || virtual_views::schema(name).is_some()
    })?;
    if name.is_empty() {
        return Err(CassieError::Planner("DROP VIEW requires a name".into()));
    }
    if catalog.get_view(&name).is_none() {
        if virtual_views::schema(&name).is_some() || catalog.exists(&name) {
            return Err(CassieError::Planner(format!(
                "relation '{name}' is not a view"
            )));
        }
        if !if_exists {
            return Err(CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::View,
                name,
            });
        }
    }
    Ok(name)
}

pub(super) fn bind_create_graph(
    mut statement: crate::sql::ast::CreateGraphStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<crate::sql::ast::CreateGraphStatement, CassieError> {
    statement.name = super::normalize_new_relation_name(statement.name.trim(), context, catalog)?;
    if statement.name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE GRAPH requires a graph name".into(),
        ));
    }

    let node_table = format!("{}_nodes", statement.name);
    let edge_table = format!("{}_edges", statement.name);
    if !statement.if_not_exists
        && (catalog.graph_exists_exact(&statement.name)
            || relation_exists_exact(catalog, &node_table)
            || relation_exists_exact(catalog, &edge_table)
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

fn relation_exists_exact(catalog: &Catalog, name: &str) -> bool {
    catalog
        .matching_relation_names(name)
        .iter()
        .any(|stored| stored.eq_ignore_ascii_case(name))
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
        if !seen.insert(field_name.to_string()) {
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
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner(
            "DROP INDEX requires an index name".into(),
        ));
    }

    if statement.table.trim().is_empty() {
        let resolved = schema_indexes::resolve_drop_index_name(&name, catalog, context)?;
        if let Some(index) = resolved {
            statement.table = index.collection;
            statement.name = index.name;
            return Ok(statement);
        }
        if statement.if_exists {
            statement.name = name;
            return Ok(statement);
        }
        return Err(CassieError::CatalogObjectNotFound {
            kind: CatalogObjectKind::Index,
            name,
        });
    }

    let table = resolve_relation_name(statement.table.trim(), catalog, context)?;
    if table.is_empty() {
        return Err(CassieError::Planner(
            "DROP INDEX requires a collection name".into(),
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

pub(super) fn bind_alter_index(
    mut statement: AlterIndexStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<AlterIndexStatement, CassieError> {
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner(
            "ALTER INDEX requires an index name".into(),
        ));
    }
    let Some(index) = schema_indexes::resolve_drop_index_name(&name, catalog, context)? else {
        if statement.if_exists {
            statement.name = name;
            return Ok(statement);
        }
        return Err(CassieError::CatalogObjectNotFound {
            kind: CatalogObjectKind::Index,
            name,
        });
    };
    if index.kind != crate::catalog::IndexKind::Scalar
        || !index.access_method().eq_ignore_ascii_case("btree")
    {
        return Err(CassieError::Unsupported(
            "ALTER INDEX storage parameters are supported only for scalar btree indexes".into(),
        ));
    }
    match &mut statement.operation {
        AlterIndexOperation::Rename { new_name } => {
            *new_name = schema_indexes::bind_index_rename_target(new_name, &index, catalog)?;
        }
        AlterIndexOperation::Set(options) => {
            let mut candidate = crate::sql::ast::CreateIndexStatement {
                name: index.name.clone(),
                table: IdentifierPath::parse(&index.collection).map_err(CassieError::Planner)?,
                fields: Vec::new(),
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                if_not_exists: false,
                unique: false,
                kind: crate::catalog::IndexKind::Scalar,
                options: std::mem::take(options),
            };
            schema_indexes::validate_scalar_storage_options(&mut candidate)?;
            *options = candidate.options;
        }
        AlterIndexOperation::Reset(options) => {
            if let Some(option) = options
                .iter()
                .find(|option| !matches!(option.as_str(), "fillfactor" | "deduplicate_items"))
            {
                return Err(CassieError::Unsupported(format!(
                    "unsupported btree storage parameter '{option}'"
                )));
            }
        }
    }
    statement.name = index.name;
    statement.collection = index.collection;
    Ok(statement)
}

pub(super) fn bind_reindex(
    mut statement: ReindexStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<ReindexStatement, CassieError> {
    match &mut statement.target {
        ReindexTarget::Index { collection, name } => {
            let raw_name = name.clone();
            let Some(index) = schema_indexes::resolve_drop_index_name(name, catalog, context)?
            else {
                return Err(CassieError::CatalogObjectNotFound {
                    kind: CatalogObjectKind::Index,
                    name: raw_name,
                });
            };
            if index.kind != crate::catalog::IndexKind::Scalar {
                return Err(CassieError::Unsupported(
                    "REINDEX INDEX currently supports scalar indexes only".to_string(),
                ));
            }
            *collection = index.collection;
            *name = index.name;
        }
        ReindexTarget::Table { name } => {
            let table = resolve_relation_name(name, catalog, context)?;
            if table.is_empty() {
                return Err(CassieError::Planner(
                    "REINDEX TABLE requires a table name".to_string(),
                ));
            }
            if !catalog.exists(&table) && !catalog.is_sql_materialized_view(&table) {
                return Err(CassieError::CollectionNotFound(table));
            }
            if catalog
                .list_indexes(&table)
                .iter()
                .any(|index| index.kind != crate::catalog::IndexKind::Scalar)
            {
                return Err(CassieError::Unsupported(
                    "REINDEX TABLE currently supports scalar indexes only".to_string(),
                ));
            }
            *name = table;
        }
    }
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
    let tables = std::iter::once(statement.table)
        .chain(statement.additional_tables)
        .map(|table| bind_drop_table_name(&table, statement.if_exists, catalog, context))
        .collect::<Result<Vec<_>, _>>()?;
    let mut tables = tables.into_iter();
    statement.table = tables
        .next()
        .ok_or_else(|| CassieError::Planner("DROP TABLE requires a table name".into()))?;
    statement.additional_tables = tables.collect();
    Ok(statement)
}

fn bind_drop_table_name(
    raw_table: &str,
    if_exists: bool,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<String, CassieError> {
    let table = if if_exists {
        super::resolve_existing_name(raw_table.trim(), context, |name| {
            catalog.relation_exists(name) || virtual_views::schema(name).is_some()
        })?
    } else {
        resolve_relation_name(raw_table.trim(), catalog, context)?
    };
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
    if !if_exists && !catalog.exists(&table) {
        return Err(CassieError::CollectionNotFound(table));
    }
    Ok(table)
}

pub(super) fn bind_alter_table(
    statement: AlterTableStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<AlterTableStatement, CassieError> {
    bind_alter_table_with_schema(statement, catalog, context, None)
}

fn bind_alter_table_with_schema(
    mut statement: AlterTableStatement,
    catalog: &Catalog,
    context: &BindingContext,
    schema_override: Option<&CollectionSchema>,
) -> Result<AlterTableStatement, CassieError> {
    let source_table = statement.table.clone();
    let table = resolve_relation_path(&statement.table, catalog, context)?;
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

    let Some(catalog_schema) = catalog.get_schema(&table) else {
        if statement.if_exists {
            statement.table = canonical_relation_path(&table)?;
            return Ok(statement);
        }
        return Err(CassieError::CollectionNotFound(table.clone()));
    };
    let schema = schema_override.cloned().unwrap_or(catalog_schema);
    if let AlterTableOperation::Multiple { operations } = &statement.operation {
        let mut bound_operations = Vec::with_capacity(operations.len());
        let mut working_schema = schema;
        for operation in operations.iter().cloned() {
            let bound = bind_alter_table_with_schema(
                AlterTableStatement {
                    table: source_table.clone(),
                    if_exists: statement.if_exists,
                    operation,
                },
                catalog,
                context,
                Some(&working_schema),
            )?;
            apply_bound_alter_to_schema(&mut working_schema, &bound.operation);
            bound_operations.push(bound.operation);
        }
        statement.table = canonical_relation_path(&table)?;
        statement.operation = AlterTableOperation::Multiple {
            operations: bound_operations,
        };
        return Ok(statement);
    }

    bind_existing_alter_table(
        &mut statement,
        &source_table,
        &table,
        &schema,
        catalog,
        context,
    )?;
    Ok(statement)
}

fn apply_bound_alter_to_schema(schema: &mut CollectionSchema, operation: &AlterTableOperation) {
    match operation {
        AlterTableOperation::AddColumn {
            field, data_type, ..
        } => {
            if !schema.fields.iter().any(|stored| {
                crate::sql::ColumnIdentifierPath::stored_field_key(&stored.name)
                    == crate::sql::ColumnIdentifierPath::stored_field_key(field)
            }) {
                schema.fields.push(crate::catalog::FieldMeta {
                    name: field.clone(),
                    data_type: data_type.clone(),
                    is_indexed: false,
                    boost: None,
                });
            }
        }
        AlterTableOperation::DropColumn { field, .. } => {
            schema.fields.retain(|stored| {
                !crate::sql::ColumnIdentifierPath::matches_stored_field(field, &stored.name)
            });
        }
        AlterTableOperation::RenameColumn { from, to } => {
            if let Some(stored) = schema.fields.iter_mut().find(|stored| {
                crate::sql::ColumnIdentifierPath::matches_stored_field(from, &stored.name)
            }) {
                stored.name.clone_from(to);
            }
        }
        AlterTableOperation::AlterColumnType {
            field, data_type, ..
        } => {
            if let Some(stored) = schema.fields.iter_mut().find(|stored| {
                crate::sql::ColumnIdentifierPath::matches_stored_field(field, &stored.name)
            }) {
                stored.data_type.clone_from(data_type);
            }
        }
        _ => {}
    }
}

fn bind_existing_alter_table(
    statement: &mut AlterTableStatement,
    source_table: &str,
    table: &str,
    schema: &CollectionSchema,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let existing_fields = schema
        .fields
        .iter()
        .map(|field| field.name.clone())
        .collect::<HashSet<_>>();
    let add_column_already_exists = match &statement.operation {
        AlterTableOperation::AddColumn {
            field,
            if_not_exists: true,
            ..
        } => existing_fields.iter().any(|existing| {
            crate::sql::ColumnIdentifierPath::stored_field_key(existing)
                == crate::sql::ColumnIdentifierPath::stored_field_key(field)
        }),
        _ => false,
    };
    if add_column_already_exists {
        statement.table = canonical_relation_path(table)?;
        return Ok(());
    }

    if let AlterTableOperation::RenameTo { table: target } = &mut statement.operation {
        *target = rename_target_in_source_schema(table, target.trim(), context)?;
    }
    if let AlterTableOperation::SetSchema { schema: target } = &mut statement.operation {
        *target = resolve_schema_name(target.trim(), catalog, context)?;
        if is_reserved_namespace(&local_name(target)) {
            return Err(CassieError::Unsupported(format!(
                "namespace '{target}' is reserved"
            )));
        }
        let source = crate::catalog::RelationId::parse_canonical(table).ok_or_else(|| {
            CassieError::Planner(format!(
                "relation '{table}' has no canonical database identity"
            ))
        })?;
        let destination = crate::catalog::SchemaId::parse_canonical(target).ok_or_else(|| {
            CassieError::Planner(format!(
                "schema '{target}' has no canonical database identity"
            ))
        })?;
        if source.database != destination.database {
            return Err(CassieError::Planner(
                "ALTER TABLE SET SCHEMA cannot move a relation between databases".into(),
            ));
        }
    }
    if let AlterTableOperation::AlterColumnSetDefault {
        default_expression,
        default_sequence: Some(sequence),
        ..
    } = &mut statement.operation
    {
        let resolved = resolve_default_sequence(sequence, table, catalog, context)?;
        *default_expression = Some(crate::catalog::canonical_nextval_expression(&resolved));
        *sequence = resolved;
    }
    if let AlterTableOperation::AddColumn {
        field, constraints, ..
    } = &mut statement.operation
    {
        requalify_alter_add_column_sequences(
            source_table,
            table,
            field,
            constraints,
            catalog,
            context,
        )?;
    }
    schema_defaults::bind_alter_defaults(&mut statement.operation, schema)?;
    validate_alter_schema(table, &statement.operation, &existing_fields, catalog)?;
    bind_alter_constraint_targets(&mut statement.operation, table, schema, catalog, context)?;
    schema_checks::bind_alter_checks(&mut statement.operation, schema)?;

    statement.table = canonical_relation_path(table)?;
    Ok(())
}

pub(super) fn validate_alter_schema(
    table: &str,
    operation: &AlterTableOperation,
    existing_fields: &HashSet<String>,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    match operation {
        AlterTableOperation::Multiple { operations } => {
            for operation in operations {
                validate_alter_schema(table, operation, existing_fields, catalog)?;
            }
        }
        AlterTableOperation::AddColumn { field, .. } => {
            validate_alter_add_column(table, field, existing_fields)?;
        }
        AlterTableOperation::AddConstraint { constraints } => {
            validate_alter_add_constraints(table, constraints, existing_fields)?;
        }
        AlterTableOperation::AddUniqueConstraintUsingIndex {
            constraint_name,
            index_name,
        } => {
            if constraint_name
                .as_ref()
                .is_some_and(|name| name.trim().is_empty())
                || index_name.trim().is_empty()
            {
                return Err(CassieError::Planner(
                    "UNIQUE USING INDEX requires a valid index name".into(),
                ));
            }
        }
        AlterTableOperation::DropConstraint { name, .. } => {
            if name.trim().is_empty() {
                return Err(CassieError::Planner(
                    "ALTER TABLE DROP CONSTRAINT requires a constraint name".into(),
                ));
            }
        }
        AlterTableOperation::DropColumn { field, .. } => {
            validate_alter_drop_column(table, field, existing_fields)?;
        }
        AlterTableOperation::RenameColumn { from, to } => {
            validate_alter_rename_column(table, from, to, existing_fields)?;
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
        AlterTableOperation::SetSchema { schema } => validate_alter_set_schema(schema)?,
        AlterTableOperation::SetAccessMethod { access_method } => {
            validate_alter_table_access_method(access_method)?;
        }
        AlterTableOperation::AlterColumnSetDefault { .. }
        | AlterTableOperation::AlterColumnDropDefault { .. }
        | AlterTableOperation::AlterColumnSetNotNull { .. }
        | AlterTableOperation::AlterColumnDropNotNull { .. }
        | AlterTableOperation::AlterColumnIdentity { .. } => {
            validate_alter_column_operation(table, operation, existing_fields, catalog)?;
        }
        AlterTableOperation::AlterColumnType { field, .. } => {
            validate_alter_column_operation(table, operation, existing_fields, catalog)?;
            if field.trim().is_empty() {
                return Err(CassieError::Planner(
                    "ALTER TABLE ALTER COLUMN TYPE requires a field".to_string(),
                ));
            }
        }
    }

    Ok(())
}

fn validate_alter_rename_column(
    table: &str,
    from: &str,
    to: &str,
    existing_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    let from = from.trim();
    let to = to.trim();
    if from.is_empty() {
        return Err(CassieError::Planner(
            "ALTER TABLE RENAME COLUMN requires a source field".into(),
        ));
    }
    if to.is_empty() {
        return Err(CassieError::Planner(
            "ALTER TABLE RENAME COLUMN requires a target field".into(),
        ));
    }
    if crate::types::row_identity::is_row_identity_column(to) {
        return Err(CassieError::Planner(
            "ALTER TABLE RENAME COLUMN cannot rename to reserved field '_id'".into(),
        ));
    }
    if crate::sql::ColumnIdentifierPath::reference_field_key(from)
        == crate::sql::ColumnIdentifierPath::stored_field_key(to)
    {
        return Err(CassieError::Planner(
            "ALTER TABLE cannot rename column to same name".into(),
        ));
    }
    if !existing_fields
        .iter()
        .any(|field| crate::sql::ColumnIdentifierPath::matches_stored_field(from, field))
    {
        return Err(CassieError::Planner(format!(
            "ALTER TABLE '{table}' has no field '{from}'"
        )));
    }
    if existing_fields.iter().any(|field| {
        crate::sql::ColumnIdentifierPath::stored_field_key(field)
            == crate::sql::ColumnIdentifierPath::stored_field_key(to)
    }) {
        return Err(CassieError::Planner(format!(
            "cannot rename column to existing field '{to}' on collection '{table}'"
        )));
    }
    Ok(())
}

fn validate_alter_table_access_method(access_method: &str) -> Result<(), CassieError> {
    if access_method == "heap" {
        return Ok(());
    }
    Err(CassieError::Unsupported(format!(
        "unsupported ALTER TABLE access method '{access_method}'"
    )))
}

fn validate_alter_set_schema(schema: &str) -> Result<(), CassieError> {
    if schema.trim().is_empty() {
        return Err(CassieError::Planner(
            "ALTER TABLE SET SCHEMA requires a schema name".into(),
        ));
    }
    Ok(())
}

fn requalify_alter_add_column_sequences(
    source_table: &str,
    bound_table: &str,
    field: &str,
    constraints: &mut [crate::catalog::FieldConstraint],
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let preserve_legacy_name = matches!(
        crate::catalog::parse_name(source_table.trim()).map_err(CassieError::Planner)?,
        crate::catalog::ParsedName::Unqualified(_)
    ) && crate::catalog::relation_schema_name(bound_table)
        .eq_ignore_ascii_case(crate::catalog::DEFAULT_SCHEMA);
    for constraint in constraints {
        let Some(sequence) = constraint.default_sequence.as_deref() else {
            continue;
        };
        let sequence = if constraint.default_sequence_owned.is_owned() {
            if preserve_legacy_name {
                continue;
            }
            crate::catalog::serial_sequence_name(bound_table, field)
        } else {
            resolve_default_sequence(sequence, bound_table, catalog, context)?
        };
        constraint.default_expression =
            Some(crate::catalog::canonical_nextval_expression(&sequence));
        constraint.default_sequence = Some(sequence);
    }
    Ok(())
}

/// `_id` is Cassie's permanently reserved internal document identity (see
/// `executor::scan::push_row_identity`); a field declared with that name
/// would be a dead column no query can ever reach.
fn validate_not_internal_identity_field(name: &str) -> Result<(), CassieError> {
    if name == "_id" {
        return Err(CassieError::Planner(
            "field '_id' conflicts with Cassie's reserved internal document identity".into(),
        ));
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
    validate_not_internal_identity_field(name)?;
    if existing_fields.iter().any(|field| {
        crate::sql::ColumnIdentifierPath::stored_field_key(field)
            == crate::sql::ColumnIdentifierPath::stored_field_key(name)
    }) {
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
        if !existing_fields
            .iter()
            .any(|field| crate::sql::ColumnIdentifierPath::matches_stored_field(name, field))
        {
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
    // `_id` is the reserved internal identity and is never a user field.
    // `id` is an ordinary column when the table declares one, so it is
    // droppable like any other; when the table does not declare it, the
    // `existing_fields` check below reports it as unknown.
    if name == "_id" {
        return Err(CassieError::Planner(format!(
            "ALTER TABLE DROP COLUMN cannot remove reserved field '{name}'"
        )));
    }
    if !existing_fields
        .iter()
        .any(|field| crate::sql::ColumnIdentifierPath::matches_stored_field(name, field))
    {
        return Err(CassieError::Planner(format!(
            "ALTER TABLE '{table}' has no field '{name}'"
        )));
    }
    Ok(())
}

/// PostgreSQL's `RENAME TO` takes an unqualified name and keeps the relation
/// in its current schema, whatever `search_path` says.
fn rename_target_in_source_schema(
    source: &str,
    target: &str,
    context: &BindingContext,
) -> Result<String, CassieError> {
    let ParsedName::Unqualified(name) = parse_name(target).map_err(CassieError::Planner)? else {
        return Err(CassieError::Parse(format!(
            "ALTER TABLE RENAME TO takes an unqualified name, got '{target}'"
        )));
    };
    if !context.scopes_database_objects() {
        return normalize_relation_name(target, context);
    }
    match parse_name(source).map_err(CassieError::Planner)? {
        ParsedName::DatabaseQualified {
            database, schema, ..
        } => Ok(canonical_relation_name(&database, &schema, &name)),
        ParsedName::SchemaQualified { schema, .. } => {
            Ok(canonical_relation_name(&context.database, &schema, &name))
        }
        ParsedName::Unqualified(_) => normalize_relation_name(&name, context),
    }
}
