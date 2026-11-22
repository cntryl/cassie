use super::command_output::empty as empty_command;
use super::{catalog, virtual_views, Cassie, FieldSchema, QueryError, QueryResult, QueryStatement};
use crate::app::{CassieError, CassieSession, CatalogObjectKind};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{
    AlterTableOperation, AlterTableStatement, CreateViewStatement, DropTableStatement,
    DropViewStatement,
};
use crate::types::DataType;

#[path = "comment_cleanup.rs"]
mod comment_cleanup;
pub(in crate::executor::execution) use comment_cleanup::{
    cleanup_relation_comments, cleanup_schema_comment,
};
#[path = "access_method_conversion.rs"]
mod access_method_conversion;
#[path = "schema_using.rs"]
mod using;

#[path = "scalar_index_rename.rs"]
mod scalar_index_rename;
#[path = "schema_constraint_helpers.rs"]
pub(super) mod schema_constraint_helpers;
#[path = "schema_foreign_keys.rs"]
pub(super) mod schema_foreign_keys;
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

    let parsed = cassie
        .parse_statement_for_session(session, &statement.query)
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
    metadata.extension_dependency_ids = selected_extension_dependency_ids(cassie, select, &context);

    metadata.definition_semantics_version = 1;
    cassie
        .midge
        .require_definition_version(metadata.definition_semantics_version)
        .map_err(QueryError::Cassie)?;
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

fn selected_extension_dependency_ids(
    cassie: &Cassie,
    select: &crate::sql::ast::SelectStatement,
    context: &crate::sql::binder::BindingContext,
) -> Vec<u64> {
    let user_functions = crate::catalog::function_resolution::functions_for_scope(
        &cassie.catalog.list_functions(),
        &context.database,
        &context.search_path,
        context.scopes_database_objects(),
    );
    let mut functions = Vec::new();
    crate::sql::binder::collect_functions(select, &mut functions);
    let mut extension_ids = std::collections::BTreeSet::new();
    for function in functions {
        let lookup = function.name.to_ascii_lowercase();
        if user_functions.get(&lookup).is_some_and(|metadata| {
            function.args.len() == metadata.args.len()
                && !crate::sql::functions::has_distinct_builtin_arity(
                    &function.name,
                    function.args.len(),
                    metadata.args.len(),
                )
        }) {
            continue;
        }
        if let Some(extension) = cassie.catalog.resolve_pgcrypto_digest(
            &context.database,
            &function.name,
            &context.search_path,
        ) {
            extension_ids.insert(extension.object_id);
        }
    }
    if crate::sql::binder::select_uses_vector_extension(select) {
        if let Some(extension) = cassie.catalog.get_extension(&context.database, "vector") {
            extension_ids.insert(extension.object_id);
        }
    }
    for relation in crate::sql::binder::select_referenced_relations(select) {
        if let Some(view) = cassie.catalog.get_view(&relation) {
            extension_ids.extend(view.extension_dependency_ids);
        }
    }
    extension_ids.into_iter().collect()
}

pub(super) fn drop_view(
    cassie: &Cassie,
    statement: &DropViewStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let output = empty_command("DROP VIEW", controls)?;
    let names = std::iter::once(&statement.name)
        .chain(&statement.additional_names)
        .map(String::as_str)
        .collect::<Vec<_>>();
    for name in &names {
        drop_view_target(cassie, name, statement.if_exists, statement.cascade, &names)?;
    }
    Ok(output)
}

fn drop_view_target(
    cassie: &Cassie,
    name: &str,
    if_exists: bool,
    cascade: bool,
    dropped_together: &[&str],
) -> Result<(), QueryError> {
    let Some(view) = cassie.catalog.get_view(name) else {
        if if_exists {
            return Ok(());
        }
        return Err(QueryError::General(format!("view '{name}' does not exist")));
    };
    let dependents = super::view_dependents::with_transitive_dependents(
        cassie,
        super::view_dependents::views_reading(cassie, &view.name),
    );
    let blocking = dependents
        .iter()
        .filter(|dependent| {
            !dropped_together.iter().any(|listed| {
                cassie
                    .catalog
                    .get_view(listed)
                    .is_some_and(|found| found.name == dependent.name)
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    if !cascade && !blocking.is_empty() {
        return Err(dependent_object_error("view", name, &blocking));
    }
    for dependent in blocking {
        // A listed view not yet reached is dropped by its own turn.
        if cassie.catalog.get_view(&dependent.name).is_some() {
            drop_stored_view(cassie, &dependent)?;
        }
    }
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
    session: Option<&CassieSession>,
    statement: &DropTableStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let output = empty_command("DROP TABLE", controls)?;
    let targets = std::iter::once(&statement.table)
        .chain(&statement.additional_tables)
        .map(String::as_str)
        .collect::<Vec<_>>();
    for table in &targets {
        drop_table_target(
            cassie,
            session,
            table,
            statement.if_exists,
            statement.cascade,
            &targets,
        )?;
    }
    Ok(output)
}

fn drop_table_target(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    if_exists: bool,
    cascade: bool,
    dropped_together: &[&str],
) -> Result<(), QueryError> {
    if if_exists && !cassie.catalog.exists(table) {
        return Ok(());
    }
    super::property_graph_dependents::reject_table_drop(cassie, table, cascade)?;
    super::type_alter::reject_row_type_dependents(cassie, table, cascade)?;
    super::sequence_default_dependents::resolve_for_table(
        cassie,
        table,
        cascade,
        dropped_together,
    )?;
    let dependent_views = super::view_dependents::with_transitive_dependents(
        cassie,
        super::view_dependents::views_reading(cassie, table),
    );
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
    let collection = cassie.catalog.get_collection(table).ok_or_else(|| {
        QueryError::Cassie(crate::app::CassieError::CatalogObjectNotFound {
            kind: crate::app::CatalogObjectKind::Relation,
            name: table.to_owned(),
        })
    })?;
    cassie
        .midge
        .defer_drop_collection(table, cassie.runtime.schema_epoch())
        .map_err(|error| QueryError::General(error.to_string()))?;
    cleanup_relation_comments(cassie, &collection.name, collection.storage_id)?;
    schema_namespace_command::cleanup_table_role_grants_after_drop(
        cassie,
        session.is_some_and(CassieSession::is_transaction_active),
        &collection.name,
        collection.storage_id,
    )?;
    schema_sequence_rename::drop_owned_table_sequences(cassie, table)?;
    if let Some(session) = session {
        // Rows staged for the dropped relation must not reach a successor
        // relation created under the same name later in this transaction.
        session.discard_staged_writes_for_dropped_relation(&collection.name);
        if collection.name != table {
            session.discard_staged_writes_for_dropped_relation(table);
        }
    }
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
    let output = empty_command("ALTER TABLE", controls)?;
    let staged_default =
        session.and_then(|session| staged_add_column_default(cassie, session, statement));
    let row_type_identity = cassie
        .catalog
        .get_collection(&statement.table)
        .map(|collection| collection.storage_id);
    super::type_alter::reject_drop_column_in_use(cassie, &statement.table, &statement.operation)?;
    execute_alter_table_operation(cassie, session, statement, controls)?;
    if let Some(storage_id) = row_type_identity {
        super::type_alter::refresh_relation_row_type(cassie, storage_id)?;
    }
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
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    let using_context = using::Context {
        cassie,
        session,
        controls,
    };
    match &statement.operation {
        AlterTableOperation::Multiple { operations } => {
            using_context.preflight(&statement.table, operations)?;
            for operation in operations {
                execute_alter_table_operation(
                    cassie,
                    session,
                    &AlterTableStatement {
                        table: statement.table.clone(),
                        if_exists: statement.if_exists,
                        operation: operation.clone(),
                    },
                    controls,
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
            session,
            &statement.table,
            &NewColumn {
                field,
                data_type,
                constraints,
                if_not_exists: *if_not_exists,
            },
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
            alter_table_drop_column(cassie, &statement.table, field, *cascade)
        }
        AlterTableOperation::RenameColumn { from, to } => {
            alter_table_rename_column(cassie, &statement.table, from, to)
        }
        AlterTableOperation::RenameTo { table } => {
            alter_table_rename_table(cassie, &statement.table, table)
        }
        AlterTableOperation::SetSchema { schema } => {
            alter_table_set_schema(cassie, session, &statement.table, schema)
        }
        AlterTableOperation::SetAccessMethod { access_method } => {
            access_method_conversion::set_access_method(
                cassie,
                &statement.table,
                access_method,
                controls,
            )
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
        operation @ (AlterTableOperation::AlterColumnDropDefault { .. }
        | AlterTableOperation::AlterColumnSetNotNull { .. }
        | AlterTableOperation::AlterColumnStorage { .. }
        | AlterTableOperation::AlterColumnDropNotNull { .. }) => {
            super::column_storage_command::alter_column_flag(cassie, &statement.table, operation)
        }
        AlterTableOperation::AlterColumnIdentity { field, action } => {
            super::identity_alter::alter_column_identity(cassie, &statement.table, field, action)
        }
        AlterTableOperation::AlterColumnType {
            field,
            data_type,
            using,
        } => {
            let table = &statement.table;
            super::property_graph_dependents::reject_column_change(cassie, table, field, false)?;
            using_context.apply(table, field, data_type, using)
        }
    }
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

/// The column of an `ALTER TABLE ... ADD COLUMN`.
struct NewColumn<'a> {
    field: &'a str,
    data_type: &'a DataType,
    constraints: &'a [crate::catalog::FieldConstraint],
    if_not_exists: bool,
}

fn alter_table_add_column(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    column: &NewColumn<'_>,
) -> Result<(), QueryError> {
    let NewColumn {
        field,
        data_type,
        constraints,
        if_not_exists,
    } = *column;
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

    let sequences =
        super::sequence_command::prepare_add_column_sequences(cassie, data_type, constraints)?;
    let default_value = constraints
        .iter()
        .find(|constraint| {
            crate::sql::ColumnIdentifierPath::stored_field_key(&constraint.field)
                == crate::sql::ColumnIdentifierPath::stored_field_key(field)
        })
        .and_then(|constraint| {
            // A volatile default such as `now()` is evaluated once and
            // backfilled into existing rows, as PostgreSQL does.
            constraint.default_value.clone().or_else(|| {
                constraint
                    .default_expression
                    .as_deref()
                    .and_then(crate::catalog::evaluate_volatile_default)
            })
        });
    let mut gated_collections = vec![table.to_string()];
    gated_collections.extend(
        constraints
            .iter()
            .filter_map(|constraint| constraint.references_table.clone()),
    );
    cassie
        .midge
        .with_collection_write_gates(&gated_collections, || {
            if !add_column_resumed(cassie, table, field)? {
                add_column_to_storage(cassie, session, table, column, default_value.clone())?;
            }

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

/// Adds the column to storage and the catalog, keeping the catalog in step with
/// storage when the backfill fails after the schema commit.
fn add_column_to_storage(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    column: &NewColumn<'_>,
    default_value: Option<serde_json::Value>,
) -> Result<(), QueryError> {
    let NewColumn {
        field,
        data_type,
        constraints,
        ..
    } = *column;
    let default_value =
        cassie.domain_add_column_default(session, table, data_type, default_value)?;
    // Existing rows receive the default, so validate the constraints
    // against that value before anything is committed.
    let backfilled = default_value.clone().unwrap_or(serde_json::Value::Null);
    let added = crate::app::AddedColumnDefault {
        field,
        value: &backfilled,
    };
    cassie.validate_existing_rows_after_added_column(table, constraints, Some(added))?;
    cassie.validate_existing_foreign_key_rows_after_added_column(
        table,
        constraints,
        Some(added),
    )?;

    let field_schema = FieldSchema {
        name: field.to_string(),
        data_type: data_type.clone(),
        nullable: true,
    };
    let altered = cassie.midge.alter_collection_add_column_with_default(
        table,
        field_schema.clone(),
        default_value,
    );
    // The schema commit precedes the backfill, so a backfill failure
    // leaves the column durable: the catalog must agree with storage.
    if altered.is_err()
        && cassie.midge.collection_schema(table).is_some_and(|schema| {
            schema
                .fields
                .iter()
                .any(|entry| entry.name == field_schema.name)
        })
    {
        cassie.catalog.add_collection_field(
            table,
            field_schema.name.clone(),
            field_schema.data_type.clone(),
        );
        cassie
            .catalog
            .register_maintenance_debt(catalog::MaintenanceDebtMeta::new(
                table,
                catalog::MaintenanceDebtMeta::add_column_backfill_artifact(field),
                0,
                0,
                altered.as_ref().err().map(ToString::to_string),
            ));
    }
    altered.map_err(|error| QueryError::General(error.to_string()))?;
    cassie
        .catalog
        .add_collection_field(table, field_schema.name, field_schema.data_type);
    Ok(())
}

/// Resumes the journaled backfill of an `ADD COLUMN` that an earlier statement
/// committed but could not finish, so retrying it completes instead of failing
/// with a duplicate column.
fn add_column_resumed(cassie: &Cassie, table: &str, field: &str) -> Result<bool, QueryError> {
    let in_catalog = cassie
        .catalog
        .get_schema(table)
        .is_some_and(|schema| schema.fields.iter().any(|entry| entry.name == field));
    if !in_catalog {
        return Ok(false);
    }
    let resumed = cassie
        .midge
        .resume_pending_field_add(table, field)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.unregister_maintenance_debt(
        table,
        &catalog::MaintenanceDebtMeta::add_column_backfill_artifact(field),
    );
    Ok(resumed)
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
) -> Result<(), QueryError> {
    super::property_graph_dependents::reject_column_change(cassie, table, field, cascade)?;
    if cascade {
        schema_foreign_keys::drop_referencing_foreign_keys_on_column(cassie, table, field)?;
    } else {
        schema_foreign_keys::reject_referenced_column_drop(cassie, table, field)?;
    }
    let dependent_views = super::view_dependents::with_transitive_dependents(
        cassie,
        super::view_dependents::views_using_column(cassie, table, field),
    );
    if !cascade && !dependent_views.is_empty() {
        return Err(dependent_object_error("column", field, &dependent_views));
    }
    for view in dependent_views {
        drop_stored_view(cassie, &view)?;
    }
    schema_constraint_helpers::drop_composite_constraints_on_column(cassie, table, field)?;
    let row_schema = cassie.midge.row_schema(table).map_err(QueryError::Cassie)?;
    let dropped_field_id = row_schema
        .fields
        .iter()
        .find(|stored| {
            !stored.retired
                && crate::sql::ColumnIdentifierPath::matches_stored_field(field, &stored.name)
        })
        .map(|stored| stored.field_id);
    let database = crate::catalog::relation_database_name(table)
        .unwrap_or_else(|| cassie.default_database.clone());
    let object_oid = cassie
        .catalog
        .collection_storage_id(table)
        .and_then(crate::catalog::object_identity::normal_oid)
        .ok_or_else(|| {
            QueryError::Cassie(crate::app::CassieError::Unsupported(
                "column comment cleanup requires a stable table catalog identity".to_owned(),
            ))
        })?;
    cassie
        .midge
        .alter_collection_drop_column(table, field)
        .map_err(|error| QueryError::General(error.to_string()))?;
    if let Some(field_id) = dropped_field_id {
        let object_sub_id = i32::try_from(field_id).map_err(|_| {
            QueryError::Cassie(crate::app::CassieError::Unsupported(
                "column attribute number exceeds PostgreSQL int4".to_owned(),
            ))
        })?;
        let address = crate::catalog::ObjectCommentAddress {
            database: database.to_ascii_lowercase(),
            class_oid: 1259,
            object_oid,
            object_sub_id,
        };
        cassie
            .midge
            .delete_object_comment(&address)
            .map_err(QueryError::Cassie)?;
        cassie
            .metadata_write_catalog
            .unregister_object_comment(&address);
    }
    cassie.catalog.remove_collection_field(table, field);
    // A column added later under the same name starts without the dropped
    // column's storage strategy and compression method.
    super::column_storage_command::reset_column_storage(cassie, table, field)?;
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
) -> Result<(), QueryError> {
    cassie
        .midge
        .alter_collection_rename_column(table, from, to)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.rename_collection_field(table, from, to);
    schema_foreign_keys::rename_referenced_field(cassie, table, from, to)?;
    super::property_graph_dependents::rename_column(cassie, table, from, to)?;
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
    cassie.catalog.reject_relation_move_target(next_table)?;
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
    super::property_graph_dependents::rename_table(cassie, table, next_table)?;
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
    cassie.catalog.reject_relation_move_target(&next_table)?;
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
    super::property_graph_dependents::rename_table(cassie, table, &next_table)?;
    schema_foreign_keys::rename_referenced_table(cassie, table, &next_table)?;
    cassie
        .bump_schema_epoch_and_invalidate_query_cache()
        .map_err(QueryError::Cassie)?;
    super::materialized_projection::mark_source_projections_stale(cassie, table)?;
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
