use super::{
    bind_select, bm25, infer_select_schema_with_context, is_reserved_namespace, local_name,
    normalize_new_relation_path, normalize_relation_name, normalize_schema_name,
    resolve_relation_name, resolve_relation_path, resolve_schema_name, select_contains_parameters,
    virtual_views, AlterSchemaOperation, AlterSchemaStatement, AlterTableOperation,
    AlterTableStatement, BindingContext, CassieError, Catalog, CatalogObjectKind, CollectionSchema,
    CreateViewStatement, DataType, DistanceMetric, DropIndexStatement, DropSchemaStatement,
    DropViewStatement, Expr, HashMap, HashSet, QueryStatement,
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
use schema_alter_constraints::{bind_alter_constraint_targets, bind_foreign_key_reference};

pub(super) fn bind_create_table(
    mut statement: crate::sql::ast::CreateTableStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<crate::sql::ast::CreateTableStatement, CassieError> {
    let name = normalize_new_relation_path(&statement.table, context, catalog)?;
    if name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE TABLE requires a table name".into(),
        ));
    }
    let already_exists = catalog.relation_exists(&name) || virtual_views::schema(&name).is_some();
    if !statement.if_not_exists && already_exists {
        return Err(CassieError::Planner(format!(
            "collection '{name}' already exists"
        )));
    }
    if statement.if_not_exists && already_exists {
        statement.table = canonical_relation_path(&name)?;
        return Ok(statement);
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

    let own_constraints = schema_checks::bind_create_checks(&mut statement.fields)?;
    let own_unique_fields = statement
        .fields
        .iter()
        .filter(|field| {
            field.constraints.iter().any(|constraint| {
                crate::catalog::enforces_single_column_uniqueness(constraint, &own_constraints)
            })
        })
        .map(|field| field.name.trim().to_string())
        .collect::<Vec<_>>();
    let mut seen = HashSet::new();
    let mut primary_key_field: Option<String> = None;
    for field in &mut statement.fields {
        let field_name = field.name.trim();
        if field_name.is_empty() {
            return Err(CassieError::Planner(
                "CREATE TABLE field names cannot be empty".into(),
            ));
        }
        validate_not_internal_identity_field(field_name)?;
        schema_defaults::bind_constraint_defaults(
            field_name,
            &field.data_type,
            &mut field.constraints,
        )?;

        if !seen.insert(field_name.to_string()) {
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
            if !bind_self_foreign_key_reference(
                constraint,
                field_name,
                &name,
                &own_unique_fields,
                context,
            )? {
                bind_foreign_key_reference(constraint, field_name, catalog, context)?;
            }
        }

        field.name = field_name.to_string();
    }

    requalify_serial_sequences(&mut statement, &name, catalog, context)?;
    statement.table = canonical_relation_path(&name)?;
    Ok(statement)
}

fn canonical_relation_path(name: &str) -> Result<IdentifierPath, CassieError> {
    IdentifierPath::parse(name).map_err(CassieError::Planner)
}

/// Binds a FOREIGN KEY that references the table being created, which is not
/// in the catalog yet, against the statement's own PRIMARY KEY and UNIQUE
/// columns. Returns `false` when the reference names another table.
fn bind_self_foreign_key_reference(
    constraint: &mut crate::catalog::FieldConstraint,
    field_label: &str,
    table: &str,
    own_unique_fields: &[String],
    context: &BindingContext,
) -> Result<bool, CassieError> {
    let (Some(referenced_table), Some(referenced_field)) = (
        constraint.references_table.as_deref(),
        constraint.references_field.as_deref(),
    ) else {
        return Ok(false);
    };
    if !normalize_relation_name(referenced_table.trim(), context)?.eq_ignore_ascii_case(table) {
        return Ok(false);
    }
    let Some(declared) = own_unique_fields.iter().find(|field| {
        crate::sql::ColumnIdentifierPath::stored_field_key(field)
            == crate::sql::ColumnIdentifierPath::reference_field_key(referenced_field)
    }) else {
        return Err(CassieError::Planner(format!(
            "foreign key on '{field_label}' must reference a primary or unique key on '{table}.{referenced_field}'"
        )));
    };
    constraint.references_field = Some(declared.clone());
    constraint.references_table = Some(table.to_string());
    Ok(true)
}

/// SERIAL sequence names are derived while parsing, before the table name is
/// qualified, so rederive them from the bound table name.
///
/// An unqualified table in the default schema keeps its legacy bare sequence
/// name, so catalogs written by earlier versions still resolve. Every other
/// table, including an unqualified one resolved through a non-default
/// `search_path` schema, gets a sequence in its own schema, which keeps
/// same-named tables in different schemas apart and lets a schema rename carry
/// the sequence with the table.
fn requalify_serial_sequences(
    statement: &mut crate::sql::ast::CreateTableStatement,
    bound_table: &str,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let preserve_legacy_serial_name = matches!(
        crate::catalog::parse_name(statement.table.trim()).map_err(CassieError::Planner)?,
        crate::catalog::ParsedName::Unqualified(_)
    ) && crate::catalog::relation_schema_name(bound_table)
        .eq_ignore_ascii_case(crate::catalog::DEFAULT_SCHEMA);
    for field in &mut statement.fields {
        for constraint in &mut field.constraints {
            let Some(sequence) = constraint.default_sequence.as_deref() else {
                continue;
            };
            let sequence = if constraint.default_sequence_owned.is_owned() {
                if preserve_legacy_serial_name {
                    continue;
                }
                crate::catalog::serial_sequence_name(bound_table, &field.name)
            } else {
                resolve_default_sequence(sequence, bound_table, catalog, context)?
            };
            constraint.default_expression =
                Some(crate::catalog::canonical_nextval_expression(&sequence));
            constraint.default_sequence = Some(sequence);
        }
    }
    Ok(())
}

fn resolve_default_sequence(
    sequence: &str,
    bound_table: &str,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<String, CassieError> {
    let sequences = catalog.list_sequences();
    let (schema, name) = match crate::catalog::parse_name(sequence).map_err(CassieError::Planner)? {
        crate::catalog::ParsedName::Unqualified(name) => {
            let table_schema = crate::catalog::relation_schema_name(bound_table);
            let candidate =
                crate::catalog::canonical_relation_name(&context.database, &table_schema, &name);
            if sequences
                .iter()
                .any(|item| item.name.eq_ignore_ascii_case(&candidate))
            {
                return Ok(candidate);
            }
            for schema in &context.search_path {
                let candidate =
                    crate::catalog::canonical_relation_name(&context.database, schema, &name);
                if sequences
                    .iter()
                    .any(|item| item.name.eq_ignore_ascii_case(&candidate))
                {
                    return Ok(candidate);
                }
            }
            return Ok(candidate);
        }
        crate::catalog::ParsedName::SchemaQualified { schema, name } => (schema, name),
        crate::catalog::ParsedName::DatabaseQualified {
            database,
            schema,
            name,
        } => {
            if !database.eq_ignore_ascii_case(&context.database) {
                return Err(CassieError::Unsupported(
                    "cross-database sequence references are not supported".to_string(),
                ));
            }
            (schema, name)
        }
    };
    Ok(crate::catalog::canonical_relation_name(
        &context.database,
        &schema,
        &name,
    ))
}

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

    let bound = bind_select(select, catalog, &HashMap::new(), context)?;
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
    let name = super::resolve_existing_name(statement.name.trim(), context, |name| {
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

    let schema = catalog
        .get_schema(&table)
        .ok_or_else(|| CassieError::CollectionNotFound(table.clone()))?;

    let existing_fields = schema
        .fields
        .iter()
        .map(|field| field.name.clone())
        .collect::<HashSet<_>>();

    if let AlterTableOperation::RenameTo { table: target } = &mut statement.operation {
        *target = rename_target_in_source_schema(&table, target.trim(), context)?;
    }
    if let AlterTableOperation::AlterColumnSetDefault {
        default_expression,
        default_sequence: Some(sequence),
        ..
    } = &mut statement.operation
    {
        let resolved = resolve_default_sequence(sequence, &table, catalog, context)?;
        *default_expression = Some(crate::catalog::canonical_nextval_expression(&resolved));
        *sequence = resolved;
    }
    if let AlterTableOperation::AddColumn {
        field, constraints, ..
    } = &mut statement.operation
    {
        requalify_alter_add_column_sequences(
            &source_table,
            &table,
            field,
            constraints,
            catalog,
            context,
        )?;
    }
    schema_defaults::bind_alter_defaults(&mut statement.operation, &schema)?;
    validate_alter_schema(&table, &statement.operation, &existing_fields, catalog)?;
    bind_alter_constraint_targets(&mut statement.operation, &schema, catalog, context)?;
    schema_checks::bind_alter_checks(&mut statement.operation, &schema)?;

    statement.table = canonical_relation_path(&table)?;
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
            constraints: _,
        } => {
            validate_alter_add_column(table, field, existing_fields)?;
        }
        AlterTableOperation::AddConstraint { constraints } => {
            validate_alter_add_constraints(table, constraints, existing_fields)?;
        }
        AlterTableOperation::DropConstraint { name, .. } => {
            if name.trim().is_empty() {
                return Err(CassieError::Planner(
                    "ALTER TABLE DROP CONSTRAINT requires a constraint name".into(),
                ));
            }
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
