use super::command_output::empty as empty_command;
use super::{catalog, virtual_views, Cassie, FieldSchema, QueryError, QueryResult, QueryStatement};
use crate::app::{CassieError, CassieSession, CatalogObjectKind};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{
    AlterTableOperation, AlterTableStatement, CreateGraphStatement, CreateViewStatement,
    DropTableStatement, DropViewStatement,
};
use crate::types::DataType;

#[path = "scalar_index_rename.rs"]
mod scalar_index_rename;
#[path = "schema_constraint_helpers.rs"]
pub(super) mod schema_constraint_helpers;
#[path = "schema_foreign_keys.rs"]
pub(super) mod schema_foreign_keys;
#[path = "schema_graph_rename.rs"]
pub(super) mod schema_graph_rename;
#[path = "schema_index_command.rs"]
mod schema_index_command;
#[path = "schema_namespace_command.rs"]
pub(super) mod schema_namespace_command;
#[path = "schema_sequence_rename.rs"]
pub(super) mod schema_sequence_rename;
pub(super) use schema_index_command::{alter_index_storage_parameters, drop_index, reindex};
pub(super) use schema_namespace_command::{
    alter_schema, create_database, create_schema, drop_database, drop_schema,
};
pub(super) fn create_graph(
    cassie: &Cassie,
    statement: &CreateGraphStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let graph = catalog::GraphMeta::new(&statement.name);
    if statement.if_not_exists && cassie.catalog.graph_exists_exact(&statement.name) {
        return empty_command("CREATE GRAPH", controls);
    }

    let output = empty_command("CREATE GRAPH", controls)?;
    super::graph_command::create_graph_collection(
        cassie,
        &graph.node_collection,
        graph.node_builtin_fields(),
        &statement.node_fields,
    )?;
    super::graph_command::create_graph_collection(
        cassie,
        &graph.edge_collection,
        graph.edge_builtin_fields(),
        &statement.edge_fields,
    )?;
    cassie
        .midge
        .put_graph(&graph)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_graph(graph);
    refresh_table_cardinality_stats(cassie, &format!("{}_nodes", statement.name))?;
    refresh_table_cardinality_stats(cassie, &format!("{}_edges", statement.name))?;

    Ok(output)
}

pub(super) fn create_view(
    cassie: &Cassie,
    session: Option<&crate::app::CassieSession>,
    statement: &CreateViewStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    cassie
        .midge
        .with_collection_gates(std::slice::from_ref(&statement.name), || {
            create_view_gated(cassie, session, statement, controls)
        })
}

fn create_view_gated(
    cassie: &Cassie,
    session: Option<&crate::app::CassieSession>,
    statement: &CreateViewStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    if statement.if_not_exists
        && (cassie.catalog.relation_exists(&statement.name)
            || virtual_views::schema(&statement.name).is_some())
    {
        return empty_command("CREATE VIEW", controls);
    }
    if cassie.catalog.relation_exists(&statement.name)
        || virtual_views::schema(&statement.name).is_some()
    {
        return Err(CassieError::CatalogObjectAlreadyExists {
            kind: CatalogObjectKind::Relation,
            name: statement.name.clone(),
        }
        .into());
    }

    let parsed = crate::sql::parser::parse_statement(&statement.query)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let context = cassie.binding_context_for_session(session);
    let bound = crate::sql::binder::bind_with_context(parsed, &cassie.catalog, &context)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let QueryStatement::Select(select) = &bound.statement.statement else {
        return Err(QueryError::General(
            "CREATE VIEW requires a SELECT query body".to_string(),
        ));
    };

    let schema =
        crate::sql::binder::infer_select_schema_with_context(select, &cassie.catalog, &context)
            .map_err(|error| QueryError::General(error.to_string()))?;
    let mut metadata =
        crate::catalog::ViewMeta::new(statement.name.clone(), statement.query.clone(), schema);

    let output = empty_command("CREATE VIEW", controls)?;
    metadata.object_id = cassie
        .midge
        .allocate_object_id()
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie
        .midge
        .put_view(&metadata)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_view(metadata);

    Ok(output)
}

pub(super) fn drop_view(
    cassie: &Cassie,
    statement: &DropViewStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let output = empty_command("DROP VIEW", controls)?;
    for name in std::iter::once(&statement.name).chain(&statement.additional_names) {
        drop_view_target(cassie, name, statement.if_exists)?;
    }
    Ok(output)
}

fn drop_view_target(cassie: &Cassie, name: &str, if_exists: bool) -> Result<(), QueryError> {
    let Some(view) = cassie.catalog.get_view(name) else {
        if if_exists {
            return Ok(());
        }
        return Err(QueryError::General(format!("view '{name}' does not exist")));
    };
    // Storage and catalog removal use exact keys, so drop the stored view name
    // rather than the name as it was typed.
    cassie
        .midge
        .defer_drop_view(&view.name, cassie.runtime.schema_epoch())
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.unregister_view(&view.name);

    Ok(())
}

pub(super) fn drop_table(
    cassie: &Cassie,
    statement: &DropTableStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let output = empty_command("DROP TABLE", controls)?;
    for table in std::iter::once(&statement.table).chain(&statement.additional_tables) {
        drop_table_target(cassie, table, statement.if_exists, statement.cascade)?;
    }
    Ok(output)
}

fn drop_table_target(
    cassie: &Cassie,
    table: &str,
    if_exists: bool,
    cascade: bool,
) -> Result<(), QueryError> {
    if if_exists && !cassie.catalog.exists(table) {
        return Ok(());
    }
    let dependent_views = views_depending_on(cassie, table, None);
    if !cascade && !dependent_views.is_empty() {
        return Err(dependent_object_error("table", table, &dependent_views));
    }
    if cascade {
        schema_foreign_keys::drop_referencing_foreign_keys_on_table(cassie, table)?;
    } else {
        schema_foreign_keys::reject_referenced_table_drop(cassie, table)?;
    }

    for view in dependent_views {
        drop_stored_view(cassie, &view)?;
    }
    cassie
        .midge
        .defer_drop_collection(table, cassie.runtime.schema_epoch())
        .map_err(|error| QueryError::General(error.to_string()))?;
    schema_sequence_rename::drop_owned_table_sequences(cassie, table)?;
    cassie
        .catalog
        .unregister_collection(table)
        .map_err(|error| QueryError::General(error.to_string()))?;

    Ok(())
}

fn drop_stored_view(cassie: &Cassie, view: &crate::catalog::ViewMeta) -> Result<(), QueryError> {
    cassie
        .midge
        .defer_drop_view(&view.name, cassie.runtime.schema_epoch())
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.unregister_view(&view.name);
    Ok(())
}

fn views_depending_on(
    cassie: &Cassie,
    relation: &str,
    column: Option<&str>,
) -> Vec<crate::catalog::ViewMeta> {
    let relation_name = relation.rsplit('.').next().unwrap_or(relation);
    let relation_token = relation_name.to_ascii_lowercase();
    let column_token = column.map(str::to_ascii_lowercase);
    cassie
        .catalog
        .list_views()
        .into_iter()
        .filter(|view| {
            let tokens = view
                .query
                .split(|character: char| {
                    character.is_whitespace() || matches!(character, '(' | ')' | ',')
                })
                .filter(|token| !token.is_empty())
                .map(|token| {
                    token
                        .trim_matches(|character| matches!(character, '"' | '`' | ';'))
                        .to_ascii_lowercase()
                })
                .collect::<Vec<_>>();
            let relation_matches_source = tokens.iter().enumerate().any(|(index, token)| {
                matches!(token.as_str(), "from" | "join")
                    && tokens
                        .get(index + 1)
                        .and_then(|source| source.rsplit('.').next())
                        .is_some_and(|source| source == relation_token)
            });
            relation_matches_source
                && column_token
                    .as_ref()
                    .is_none_or(|column| tokens.contains(column))
        })
        .collect()
}

fn dependent_object_error(
    kind: &str,
    name: &str,
    views: &[crate::catalog::ViewMeta],
) -> QueryError {
    let display_name = name.rsplit('.').next().unwrap_or(name);
    let dependent = views
        .iter()
        .map(|view| format!("view {}", view.name))
        .collect::<Vec<_>>()
        .join(", ");
    QueryError::Cassie(
        crate::app::DiagnosticError::new(
            "2BP01",
            format!("cannot drop {kind} {display_name} because other objects depend on it"),
        )
        .with_detail(format!("dependent objects: {dependent}"))
        .into(),
    )
}

pub(super) fn alter_table(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &AlterTableStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    if statement.if_exists && !cassie.catalog.exists(&statement.table) {
        return empty_command("ALTER TABLE", controls);
    }
    let is_column_store = cassie
        .catalog
        .collection_storage_mode(&statement.table)
        .is_some_and(crate::catalog::collections::CollectionStorageMode::uses_column_store_storage);
    let output = empty_command("ALTER TABLE", controls)?;
    let staged_default =
        session.and_then(|session| staged_add_column_default(cassie, session, statement));
    execute_alter_table_operation(cassie, session, statement, is_column_store)?;
    if let (Some(session), Some((field, default_value))) = (session, staged_default) {
        session
            .apply_literal_default_to_staged_rows(&statement.table, &field, &default_value)
            .map_err(QueryError::Cassie)?;
    }
    if let Some(session) = session {
        match &statement.operation {
            AlterTableOperation::RenameTo { table: next_table } => session
                .rename_transaction_collections(&[(
                    statement.table.as_str().to_owned(),
                    next_table.clone(),
                )])
                .map_err(QueryError::Cassie)?,
            AlterTableOperation::RenameColumn { from, to } => {
                session.rename_transaction_column(statement.table.as_str(), from, to);
            }
            _ => {}
        }
    }

    Ok(output)
}

fn execute_alter_table_operation(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &AlterTableStatement,
    is_column_store: bool,
) -> Result<(), QueryError> {
    match &statement.operation {
        AlterTableOperation::Multiple { operations } => {
            preflight_alter_table_using(cassie, session, &statement.table, operations)?;
            for operation in operations {
                execute_alter_table_operation(
                    cassie,
                    session,
                    &AlterTableStatement {
                        table: statement.table.clone(),
                        if_exists: statement.if_exists,
                        operation: operation.clone(),
                    },
                    is_column_store,
                )?;
            }
            Ok(())
        }
        AlterTableOperation::AddColumn {
            field,
            data_type,
            constraints,
            if_not_exists,
        } => alter_table_add_column(
            cassie,
            &statement.table,
            field,
            data_type,
            constraints,
            *if_not_exists,
            is_column_store,
        ),
        AlterTableOperation::AddConstraint { constraints } => {
            alter_table_add_constraint(cassie, &statement.table, constraints)
        }
        AlterTableOperation::AddUniqueConstraintUsingIndex { .. } => Err(QueryError::General(
            "UNIQUE USING INDEX was not resolved by the SQL binder".to_string(),
        )),
        AlterTableOperation::DropConstraint { name, if_exists } => {
            alter_table_drop_constraint(cassie, &statement.table, name, *if_exists)
        }
        AlterTableOperation::DropColumn { field, cascade } => {
            alter_table_drop_column(cassie, &statement.table, field, *cascade, is_column_store)
        }
        AlterTableOperation::RenameColumn { from, to } => {
            alter_table_rename_column(cassie, &statement.table, from, to, is_column_store)
        }
        AlterTableOperation::RenameTo { table } => {
            alter_table_rename_table(cassie, &statement.table, table)
        }
        AlterTableOperation::SetSchema { schema } => {
            alter_table_set_schema(cassie, session, &statement.table, schema)
        }
        AlterTableOperation::SetAccessMethod { access_method } => {
            if access_method == "heap" {
                Ok(())
            } else {
                Err(QueryError::Cassie(crate::app::CassieError::Unsupported(
                    format!("unsupported ALTER TABLE access method '{access_method}'"),
                )))
            }
        }
        AlterTableOperation::AlterColumnSetDefault {
            field,
            default_value,
            default_expression,
            default_sequence,
        } => super::sequence_command::alter_column_set_default(
            cassie,
            &statement.table,
            field,
            default_value.clone(),
            default_expression.clone(),
            default_sequence.clone(),
        ),
        AlterTableOperation::AlterColumnDropDefault { field } => {
            super::sequence_command::alter_column_drop_default(cassie, &statement.table, field)
        }
        AlterTableOperation::AlterColumnSetNotNull { field } => {
            super::sequence_command::alter_column_set_not_null(cassie, &statement.table, field)
        }
        AlterTableOperation::AlterColumnDropNotNull { field } => {
            super::sequence_command::alter_column_drop_not_null(cassie, &statement.table, field)
        }
        AlterTableOperation::AlterColumnIdentity { field, action } => {
            super::identity_alter::alter_column_identity(cassie, &statement.table, field, action)
        }
        AlterTableOperation::AlterColumnType {
            field,
            data_type,
            using,
        } => alter_table_column_type(
            cassie,
            session,
            &statement.table,
            field,
            data_type,
            using,
            is_column_store,
        ),
    }
}

fn preflight_alter_table_using(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    operations: &[AlterTableOperation],
) -> Result<(), QueryError> {
    for operation in operations {
        if let AlterTableOperation::AlterColumnType { field, using, .. } = operation {
            evaluate_alter_column_using(cassie, session, table, field, using)?;
        }
    }
    Ok(())
}

fn evaluate_alter_column_using(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    field: &str,
    expression: &crate::sql::ast::Expr,
) -> Result<Vec<(String, serde_json::Value)>, QueryError> {
    let schema = cassie
        .catalog
        .get_schema(table)
        .ok_or_else(|| QueryError::Cassie(CassieError::CollectionNotFound(table.to_string())))?;
    let documents = cassie
        .midge
        .scan_documents(table)
        .map_err(QueryError::Cassie)?;
    let empty_functions = std::collections::HashMap::new();
    documents
        .into_iter()
        .map(|document| {
            let row = super::super::scan::document_to_row(&document, Some(&schema));
            let value = crate::executor::filter::evaluate_expr_value(
                &row,
                expression,
                &[],
                None,
                &empty_functions,
                session,
                None,
            )?;
            let json = crate::types::json::try_value_to_json(value)
                .map_err(|error| QueryError::General(error.to_string()))?;
            let mut payload = document.payload;
            let object = payload
                .as_object_mut()
                .ok_or_else(|| QueryError::General(format!("row in '{table}' is not an object")))?;
            object.insert(field.to_string(), json);
            Ok((document.id, payload))
        })
        .collect()
}

fn alter_table_column_type(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    field: &str,
    data_type: &DataType,
    using: &crate::sql::ast::Expr,
    is_column_store: bool,
) -> Result<(), QueryError> {
    ensure_row_store_alter_supported(is_column_store, "ALTER COLUMN TYPE")?;
    let converted = evaluate_alter_column_using(cassie, session, table, field, using)?;
    for (id, _) in &converted {
        cassie
            .midge
            .delete_document(table, id)
            .map_err(QueryError::Cassie)?;
    }
    cassie
        .midge
        .alter_collection_column_type(table, field, data_type.clone())
        .map_err(QueryError::Cassie)?;
    if !converted.is_empty() {
        cassie
            .midge
            .put_documents(
                table,
                converted
                    .into_iter()
                    .map(|(id, payload)| (Some(id), payload))
                    .collect(),
            )
            .map_err(QueryError::Cassie)?;
    }
    cassie
        .catalog
        .set_collection_field_type(table, field, data_type.clone());
    cassie
        .bump_schema_epoch_and_invalidate_query_cache()
        .map_err(QueryError::Cassie)?;
    refresh_table_cardinality_stats(cassie, table)
}

fn staged_add_column_default(
    cassie: &Cassie,
    session: &CassieSession,
    statement: &AlterTableStatement,
) -> Option<(String, serde_json::Value)> {
    if !session.is_transaction_active() {
        return None;
    }
    let AlterTableOperation::AddColumn {
        field,
        constraints,
        if_not_exists,
        ..
    } = &statement.operation
    else {
        return None;
    };
    if *if_not_exists
        && cassie
            .catalog
            .get_schema(&statement.table)
            .is_some_and(|schema| {
                schema.fields.iter().any(|existing| {
                    crate::sql::ColumnIdentifierPath::stored_field_key(&existing.name)
                        == crate::sql::ColumnIdentifierPath::stored_field_key(field)
                })
            })
    {
        return None;
    }
    constraints
        .iter()
        .find(|constraint| {
            crate::sql::ColumnIdentifierPath::stored_field_key(&constraint.field)
                == crate::sql::ColumnIdentifierPath::stored_field_key(field)
        })
        .and_then(|constraint| constraint.default_value.clone())
        .map(|default_value| (field.clone(), default_value))
}

fn alter_table_add_column(
    cassie: &Cassie,
    table: &str,
    field: &str,
    data_type: &DataType,
    constraints: &[crate::catalog::FieldConstraint],
    if_not_exists: bool,
    is_column_store: bool,
) -> Result<(), QueryError> {
    if if_not_exists
        && cassie.catalog.get_schema(table).is_some_and(|schema| {
            schema.fields.iter().any(|existing| {
                crate::sql::ColumnIdentifierPath::stored_field_key(&existing.name)
                    == crate::sql::ColumnIdentifierPath::stored_field_key(field)
            })
        })
    {
        return Ok(());
    }

    ensure_row_store_alter_supported(is_column_store, "ALTER TABLE ADD COLUMN")?;
    let sequences =
        super::sequence_command::prepare_add_column_sequences(cassie, data_type, constraints)?;
    let default_value = constraints
        .iter()
        .find(|constraint| {
            crate::sql::ColumnIdentifierPath::stored_field_key(&constraint.field)
                == crate::sql::ColumnIdentifierPath::stored_field_key(field)
        })
        .and_then(|constraint| constraint.default_value.clone());
    let mut gated_collections = vec![table.to_string()];
    gated_collections.extend(
        constraints
            .iter()
            .filter_map(|constraint| constraint.references_table.clone()),
    );
    cassie
        .midge
        .with_collection_write_gates(&gated_collections, || {
            cassie.validate_existing_check_and_not_null_rows(table, constraints)?;
            cassie.validate_existing_foreign_key_rows(table, constraints)?;

            let field_schema = FieldSchema {
                name: field.to_string(),
                data_type: data_type.clone(),
                nullable: true,
            };
            cassie
                .midge
                .alter_collection_add_column_with_default(
                    table,
                    field_schema.clone(),
                    default_value.clone(),
                )
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie
                .catalog
                .add_collection_field(table, field_schema.name, field_schema.data_type);

            super::sequence_command::persist_created_sequences(cassie, sequences)?;
            if !constraints.is_empty() {
                let mut merged = cassie.catalog.get_constraints(table);
                crate::catalog::merge_constraint_set(&mut merged, constraints.to_vec());
                cassie
                    .midge
                    .save_constraints_with_unique_reservations(table, merged.as_slice())
                    .map_err(|error| QueryError::General(error.to_string()))?;
                cassie.catalog.register_constraints(table, merged);
            }
            refresh_table_cardinality_stats(cassie, table)
        })?;
    Ok(())
}

fn alter_table_add_constraint(
    cassie: &Cassie,
    table: &str,
    constraints: &[crate::catalog::FieldConstraint],
) -> Result<(), QueryError> {
    let mut additions = constraints.to_vec();
    let mut gated_collections = vec![table.to_string()];
    gated_collections.extend(
        additions
            .iter()
            .filter_map(|constraint| constraint.references_table.clone()),
    );
    cassie
        .midge
        .with_collection_write_gates(&gated_collections, || {
            let existing = cassie.catalog.get_constraints(table);
            schema_constraint_helpers::name_composite_primary_key(table, &mut additions);
            if additions.iter().any(|constraint| constraint.primary_key)
                && existing.iter().any(|constraint| constraint.primary_key)
            {
                return Err(QueryError::General(format!(
                    "multiple primary keys defined on '{table}'"
                )));
            }
            if let Some(message) =
                crate::catalog::conflicting_unique_membership(table, &existing, &additions)
            {
                return Err(QueryError::General(message));
            }
            cassie.validate_existing_check_and_not_null_rows(table, &additions)?;
            cassie.validate_existing_foreign_key_rows(table, &additions)?;
            let attached_index_name =
                schema_constraint_helpers::validate_constraint_index_attachment(
                    cassie,
                    table,
                    &existing,
                    &mut additions,
                )?;
            let mut merged = existing;
            crate::catalog::merge_constraint_set(&mut merged, additions.clone());
            let mut unique_indexes = super::primary_key_indexes(table, &additions);
            unique_indexes.extend(crate::catalog::unique_constraint_indexes(table, &additions));
            unique_indexes.extend(crate::catalog::composite_primary_key_indexes(
                table, &additions,
            ));
            if let Some(source_index_name) = attached_index_name {
                let constraint_name = additions
                    .iter()
                    .find(|constraint| constraint.unique)
                    .map(|constraint| constraint.unique_constraint_name(table))
                    .ok_or_else(|| {
                        QueryError::General(
                            "UNIQUE USING INDEX has no unique constraint fields".to_string(),
                        )
                    })?;
                let adopted_index = cassie
                    .midge
                    .rename_index_and_constraints_with_held_collection_gate(
                        table,
                        &source_index_name,
                        &constraint_name,
                        merged.as_slice(),
                    )
                    .map_err(QueryError::from)?;
                cassie.catalog.unregister_index(table, &source_index_name);
                cassie.catalog.register_index(adopted_index);
            } else {
                for index in &unique_indexes {
                    cassie
                        .midge
                        .put_index_with_held_collection_gate(index)
                        .map_err(QueryError::from)?;
                }
                cassie
                    .midge
                    .save_constraints_with_unique_reservations(table, merged.as_slice())
                    .map_err(|error| QueryError::General(error.to_string()))?;
            }
            cassie.catalog.register_constraints(table, merged);
            for index in unique_indexes {
                cassie.catalog.register_index(index);
            }
            Ok(())
        })
}

fn alter_table_drop_constraint(
    cassie: &Cassie,
    table: &str,
    name: &str,
    if_exists: bool,
) -> Result<(), QueryError> {
    schema_constraint_helpers::alter_table_drop_constraint(cassie, table, name, if_exists)
}

fn alter_table_drop_column(
    cassie: &Cassie,
    table: &str,
    field: &str,
    cascade: bool,
    is_column_store: bool,
) -> Result<(), QueryError> {
    ensure_row_store_alter_supported(is_column_store, "ALTER TABLE DROP COLUMN")?;
    schema_graph_rename::reject_graph_column_drop(cassie, table, field)?;
    if cascade {
        schema_foreign_keys::drop_referencing_foreign_keys_on_column(cassie, table, field)?;
    } else {
        schema_foreign_keys::reject_referenced_column_drop(cassie, table, field)?;
    }
    let dependent_views = views_depending_on(cassie, table, Some(field));
    if !cascade && !dependent_views.is_empty() {
        return Err(dependent_object_error("column", field, &dependent_views));
    }
    for view in dependent_views {
        drop_stored_view(cassie, &view)?;
    }
    schema_constraint_helpers::drop_composite_constraints_on_column(cassie, table, field)?;
    cassie
        .midge
        .alter_collection_drop_column(table, field)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.remove_collection_field(table, field);
    unregister_vector_indexes_on_column(cassie, table, field);
    schema_foreign_keys::drop_foreign_keys_on_column(cassie, table, field)?;
    cassie
        .bump_schema_epoch_and_invalidate_query_cache()
        .map_err(QueryError::Cassie)?;
    super::materialized_projection::mark_source_projections_stale(cassie, table)?;
    refresh_table_cardinality_stats(cassie, table)
}

fn unregister_vector_indexes_on_column(cassie: &Cassie, table: &str, field: &str) {
    for index in cassie.catalog.list_indexes(table) {
        if matches!(index.kind, catalog::IndexKind::Vector) && index.references_field(field) {
            cassie
                .catalog
                .unregister_index(&index.collection, &index.name);
        }
    }
    for index in cassie.catalog.list_vector_indexes(table) {
        if crate::sql::ColumnIdentifierPath::stored_field_key(&index.field)
            == crate::sql::ColumnIdentifierPath::stored_field_key(field)
            || crate::sql::ColumnIdentifierPath::stored_field_key(&index.source_field)
                == crate::sql::ColumnIdentifierPath::stored_field_key(field)
        {
            cassie
                .catalog
                .unregister_vector_index(&index.collection, &index.field);
        }
    }
}

fn alter_table_rename_column(
    cassie: &Cassie,
    table: &str,
    from: &str,
    to: &str,
    is_column_store: bool,
) -> Result<(), QueryError> {
    ensure_row_store_alter_supported(is_column_store, "ALTER TABLE RENAME COLUMN")?;
    cassie
        .midge
        .alter_collection_rename_column(table, from, to)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.rename_collection_field(table, from, to);
    schema_foreign_keys::rename_referenced_field(cassie, table, from, to)?;
    schema_graph_rename::rename_graph_columns(cassie, table, from, to)?;
    cassie
        .bump_schema_epoch_and_invalidate_query_cache()
        .map_err(QueryError::Cassie)?;
    super::materialized_projection::mark_source_projections_stale(cassie, table)?;
    refresh_table_cardinality_stats(cassie, table)
}

fn alter_table_rename_table(
    cassie: &Cassie,
    table: &str,
    next_table: &str,
) -> Result<(), QueryError> {
    if cassie.catalog.exists(next_table) {
        return Err(QueryError::General(format!(
            "collection '{next_table}' already exists"
        )));
    }
    cassie
        .midge
        .rename_collection(table, next_table)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie
        .catalog
        .rename_collection(table, next_table)
        .map_err(|error| QueryError::General(error.to_string()))?;
    schema_sequence_rename::rename_owned_table_sequences(cassie, next_table)?;
    schema_graph_rename::rename_graph_tables(cassie, table, next_table)?;
    schema_foreign_keys::rename_referenced_table(cassie, table, next_table)
}

fn alter_table_set_schema(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    target_schema: &str,
) -> Result<(), QueryError> {
    let relation = crate::catalog::RelationId::parse_canonical(table).ok_or_else(|| {
        QueryError::General(format!(
            "relation '{table}' has no canonical database identity"
        ))
    })?;
    let schema = crate::catalog::SchemaId::parse_canonical(target_schema).ok_or_else(|| {
        QueryError::General(format!(
            "schema '{target_schema}' has no canonical database identity"
        ))
    })?;
    let next_table =
        crate::catalog::RelationId::new(relation.database.clone(), schema.schema, relation.name)
            .canonical_name();
    if table == next_table {
        return Ok(());
    }
    if cassie.catalog.exists(&next_table) {
        return Err(QueryError::General(format!(
            "collection '{next_table}' already exists"
        )));
    }
    let sequence_move =
        schema_sequence_rename::prepare_owned_table_sequence_move(cassie, table, &next_table)?;

    cassie
        .midge
        .rename_collection(table, &next_table)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie
        .catalog
        .rename_collection(table, &next_table)
        .map_err(QueryError::Cassie)?;
    if let Some(session) = session {
        session
            .rename_transaction_collections(&[(table.to_owned(), next_table.clone())])
            .map_err(QueryError::Cassie)?;
    }
    schema_sequence_rename::move_owned_table_sequences(cassie, sequence_move)?;
    schema_graph_rename::rename_graph_tables(cassie, table, &next_table)?;
    schema_foreign_keys::rename_referenced_table(cassie, table, &next_table)?;
    cassie
        .bump_schema_epoch_and_invalidate_query_cache()
        .map_err(QueryError::Cassie)?;
    super::materialized_projection::mark_source_projections_stale(cassie, table)?;
    Ok(())
}

fn ensure_row_store_alter_supported(
    is_column_store: bool,
    operation: &str,
) -> Result<(), QueryError> {
    if is_column_store {
        return Err(QueryError::General(format!(
            "{operation} is not supported for column-store tables"
        )));
    }
    Ok(())
}

pub(super) fn refresh_table_cardinality_stats(
    cassie: &Cassie,
    table: &str,
) -> Result<(), QueryError> {
    cassie
        .refresh_cardinality_stats(table)
        .map_err(|error| QueryError::General(error.to_string()))
}
