use super::{
    catalog, check_timeout, filter, primary_key_indexes, virtual_views, Cassie, CassieSession,
    FieldSchema, FunctionMeta, HashMap, LogicalCommand, ProcedureMeta, QueryError,
    QueryExecutionControls, QueryResult, QueryStatement, Schema, Value, Volatility,
};

pub(super) fn execute_command(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    command: &LogicalCommand,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    check_timeout(controls)?;
    let outcome = command_outcome(cassie, session, command, params, user_functions, controls);

    if outcome.invalidate_plan_cache {
        cassie
            .bump_schema_epoch_and_invalidate_query_cache()
            .map_err(|error| QueryError::General(error.to_string()))?;
    }

    outcome.result
}

fn command_outcome(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    command: &LogicalCommand,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> CommandExecution {
    execute_session_or_dml_command(cassie, session, command, params, user_functions, controls)
        .or_else(|| execute_projection_command_group(cassie, command, user_functions, controls))
        .or_else(|| execute_retention_sequence_group(cassie, command, user_functions, controls))
        .or_else(|| execute_schema_object_group(cassie, command))
        .or_else(|| {
            execute_routine_group(cassie, session, command, params, user_functions, controls)
        })
        .expect("all logical commands should be handled")
}

fn execute_session_or_dml_command(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    command: &LogicalCommand,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Option<CommandExecution> {
    match command {
        LogicalCommand::Show(statement) => Some(CommandExecution::new(
            super::session_command::execute_show(statement),
        )),
        LogicalCommand::Set(statement) => Some(CommandExecution::new(
            super::session_command::execute_set(statement),
        )),
        LogicalCommand::Copy(_) => Some(CommandExecution::new(Err(QueryError::General(
            "COPY requires pgwire COPY FROM STDIN data stream".to_string(),
        )))),
        LogicalCommand::Insert(statement) => Some(execute_insert_command(
            cassie,
            session,
            statement,
            params,
            user_functions,
            controls,
        )),
        LogicalCommand::Update(statement) => Some(execute_update_command(
            cassie,
            session,
            statement,
            params,
            user_functions,
            controls,
        )),
        LogicalCommand::Delete(statement) => Some(execute_delete_command(
            cassie,
            session,
            statement,
            params,
            user_functions,
            controls,
        )),
        _ => None,
    }
}

fn execute_projection_command_group(
    cassie: &Cassie,
    command: &LogicalCommand,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Option<CommandExecution> {
    match command {
        LogicalCommand::CreateRollup(statement) => Some(CommandExecution::invalidating(
            super::rollups::create_rollup(cassie, statement, user_functions, controls),
        )),
        LogicalCommand::RefreshRollup(statement) => Some(CommandExecution::invalidating(
            super::rollups::refresh_rollup(cassie, &statement.name, user_functions, controls),
        )),
        LogicalCommand::DropRollup(statement) => Some(CommandExecution::invalidating(
            super::rollups::drop_rollup(cassie, &statement.name, statement.if_exists),
        )),
        LogicalCommand::CreateMaterializedProjection(statement) => {
            Some(CommandExecution::invalidating(
                super::materialized_projection::create_materialized_projection(
                    cassie,
                    statement,
                    user_functions,
                    controls,
                ),
            ))
        }
        LogicalCommand::RefreshMaterializedProjection(statement) => {
            Some(CommandExecution::invalidating(
                super::materialized_projection::refresh_materialized_projection(
                    cassie,
                    &statement.name,
                    user_functions,
                    controls,
                ),
            ))
        }
        LogicalCommand::DropMaterializedProjection(statement) => {
            Some(CommandExecution::invalidating(
                super::materialized_projection::drop_materialized_projection(
                    cassie,
                    &statement.name,
                    statement.if_exists,
                ),
            ))
        }
        LogicalCommand::AlterMaterializedProjection(statement) => {
            Some(CommandExecution::invalidating(
                super::materialized_projection::alter_materialized_projection(
                    cassie,
                    statement,
                    user_functions,
                    controls,
                ),
            ))
        }
        LogicalCommand::DropMaterializedProjectionVersion(statement) => {
            Some(CommandExecution::invalidating(
                super::materialized_projection::drop_materialized_projection_version(
                    cassie,
                    &statement.name,
                    &statement.version_id,
                ),
            ))
        }
        LogicalCommand::VerifyProjection(statement) => Some(CommandExecution::new(
            super::materialized_projection::verify_projection(cassie, statement),
        )),
        LogicalCommand::DiffProjection(statement) => Some(CommandExecution::new(
            super::projection_diff::diff_projection(cassie, statement),
        )),
        LogicalCommand::CompareProjection(statement) => Some(CommandExecution::new(
            super::projection_diff::compare_projection(cassie, statement),
        )),
        LogicalCommand::PlanRepairProjection(statement) => Some(CommandExecution::new(
            super::projection_repair::plan_repair_projection(
                cassie,
                &statement.target,
                statement.scope,
            ),
        )),
        LogicalCommand::RepairProjection(statement) => Some(CommandExecution::new(
            super::projection_repair::repair_projection(cassie, statement),
        )),
        _ => None,
    }
}

fn execute_retention_sequence_group(
    cassie: &Cassie,
    command: &LogicalCommand,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Option<CommandExecution> {
    match command {
        LogicalCommand::CreateRetentionPolicy(statement) => Some(CommandExecution::invalidating(
            super::retention::create_retention_policy(cassie, statement),
        )),
        LogicalCommand::AlterRetentionPolicy(statement) => Some(CommandExecution::invalidating(
            super::retention::alter_retention_policy(cassie, statement),
        )),
        LogicalCommand::DropRetentionPolicy(statement) => Some(CommandExecution::invalidating(
            super::retention::drop_retention_policy(cassie, &statement.name, statement.if_exists),
        )),
        LogicalCommand::EnforceRetentionPolicy(statement) => Some(CommandExecution::new(
            super::retention::enforce_retention_policy(cassie, statement, user_functions, controls),
        )),
        LogicalCommand::CreateSequence(statement) => Some(CommandExecution::invalidating(
            super::sequence_command::create_sequence(cassie, statement),
        )),
        LogicalCommand::DropSequence(statement) => Some(CommandExecution::invalidating(
            super::sequence_command::drop_sequence(cassie, statement),
        )),
        _ => None,
    }
}

fn execute_schema_object_group(
    cassie: &Cassie,
    command: &LogicalCommand,
) -> Option<CommandExecution> {
    match command {
        LogicalCommand::CreateTable(statement) => {
            Some(execute_create_table_command(cassie, statement))
        }
        LogicalCommand::CreateGraph(statement) => {
            Some(execute_create_graph_command(cassie, statement))
        }
        LogicalCommand::CreateView(statement) => {
            Some(execute_create_view_command(cassie, statement))
        }
        LogicalCommand::DropView(statement) => Some(execute_drop_view_command(cassie, statement)),
        LogicalCommand::DropTable(statement) => Some(execute_drop_table_command(cassie, statement)),
        LogicalCommand::AlterTable(statement) => {
            Some(execute_alter_table_command(cassie, statement))
        }
        LogicalCommand::CreateSchema(statement) => {
            Some(execute_create_schema_command(cassie, statement))
        }
        LogicalCommand::DropSchema(statement) => {
            Some(execute_drop_schema_command(cassie, statement))
        }
        LogicalCommand::AlterSchema(statement) => {
            Some(execute_alter_schema_command(cassie, statement))
        }
        LogicalCommand::CreateRole(statement) => {
            Some(execute_create_role_command(cassie, statement))
        }
        LogicalCommand::AlterRole(statement) => Some(execute_alter_role_command(cassie, statement)),
        LogicalCommand::DropRole(statement) => Some(execute_drop_role_command(cassie, statement)),
        LogicalCommand::CreateIndex(statement) => {
            Some(execute_create_index_command(cassie, statement))
        }
        LogicalCommand::DropIndex(statement) => Some(execute_drop_index_command(cassie, statement)),
        _ => None,
    }
}

fn execute_routine_group(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    command: &LogicalCommand,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Option<CommandExecution> {
    match command {
        LogicalCommand::CreateFunction(statement) => {
            Some(execute_create_function_command(cassie, statement))
        }
        LogicalCommand::DropFunction(statement) => {
            Some(execute_drop_function_command(cassie, statement))
        }
        LogicalCommand::CreateProcedure(statement) => {
            Some(execute_create_procedure_command(cassie, statement))
        }
        LogicalCommand::DropProcedure(statement) => {
            Some(execute_drop_procedure_command(cassie, statement))
        }
        LogicalCommand::CallProcedure(statement) => Some(execute_call_procedure_command(
            cassie,
            session,
            statement,
            params,
            user_functions,
            controls,
        )),
        _ => None,
    }
}

struct CommandExecution {
    result: Result<QueryResult, QueryError>,
    invalidate_plan_cache: bool,
}

impl CommandExecution {
    fn new(result: Result<QueryResult, QueryError>) -> Self {
        Self {
            result,
            invalidate_plan_cache: false,
        }
    }

    fn invalidating(result: Result<QueryResult, QueryError>) -> Self {
        Self {
            result,
            invalidate_plan_cache: true,
        }
    }
}

fn execute_create_table_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateTableStatement,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        if statement.if_not_exists
            && (cassie.catalog.relation_exists(&statement.table)
                || virtual_views::schema(&statement.table).is_some())
        {
            return Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE TABLE".to_string(),
            });
        }

        let schema = Schema {
            fields: statement
                .fields
                .iter()
                .map(|field| FieldSchema {
                    name: field.name.clone(),
                    data_type: field.data_type.clone(),
                    nullable: true,
                })
                .collect(),
        };
        let collection_meta = catalog::CollectionMeta::new_with_storage_mode(
            &statement.table,
            None,
            statement.storage_mode,
        );
        let table_sequences =
            super::sequence_command::prepare_create_table_sequences(cassie, statement)?;

        cassie
            .midge
            .create_collection_with_meta(&statement.table, schema.clone(), collection_meta.clone())
            .map_err(|error| QueryError::General(error.to_string()))?;

        let constraints = statement
            .fields
            .iter()
            .flat_map(|field| field.constraints.iter().cloned())
            .collect::<Vec<_>>();

        cassie
            .midge
            .save_constraints(&statement.table, constraints.as_slice())
            .map_err(|error| QueryError::General(error.to_string()))?;
        let primary_key_indexes = primary_key_indexes(&statement.table, constraints.as_slice());
        for index in &primary_key_indexes {
            cassie
                .midge
                .put_index(index)
                .map_err(|error| QueryError::General(error.to_string()))?;
        }
        super::sequence_command::persist_created_sequences(cassie, table_sequences)?;
        cassie.catalog.register_collection_meta_with_constraints(
            collection_meta,
            schema
                .fields
                .into_iter()
                .map(|field| (field.name, field.data_type))
                .collect(),
            constraints,
        );
        for index in primary_key_indexes {
            cassie.catalog.register_index(index);
        }
        cassie
            .refresh_cardinality_stats(&statement.table)
            .map_err(|error| QueryError::General(error.to_string()))?;

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: "CREATE TABLE".to_string(),
        })
    })())
}

fn execute_create_graph_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateGraphStatement,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        let graph = catalog::GraphMeta::new(&statement.name);
        if statement.if_not_exists && cassie.catalog.graph_exists(&statement.name) {
            return Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE GRAPH".to_string(),
            });
        }

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
        cassie
            .refresh_cardinality_stats(&format!("{}_nodes", statement.name))
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie
            .refresh_cardinality_stats(&format!("{}_edges", statement.name))
            .map_err(|error| QueryError::General(error.to_string()))?;

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: "CREATE GRAPH".to_string(),
        })
    })())
}

fn execute_create_view_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateViewStatement,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        if statement.if_not_exists
            && (cassie.catalog.relation_exists(&statement.name)
                || virtual_views::schema(&statement.name).is_some())
        {
            return Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE VIEW".to_string(),
            });
        }

        let parsed = crate::sql::parser::parse_statement(&statement.query)
            .map_err(|error| QueryError::General(error.0))?;
        let bound = crate::sql::binder::bind(parsed, &cassie.catalog)
            .map_err(|error| QueryError::General(error.to_string()))?;
        let QueryStatement::Select(select) = &bound.statement.statement else {
            return Err(QueryError::General(
                "CREATE VIEW requires a SELECT query body".to_string(),
            ));
        };

        let schema = crate::sql::binder::infer_select_schema(select, &cassie.catalog)
            .map_err(|error| QueryError::General(error.to_string()))?;
        let metadata =
            crate::catalog::ViewMeta::new(statement.name.clone(), statement.query.clone(), schema);

        cassie
            .midge
            .put_view(&metadata)
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie.catalog.register_view(metadata);

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: "CREATE VIEW".to_string(),
        })
    })())
}

fn execute_drop_view_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::DropViewStatement,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        let view = cassie.catalog.get_view(&statement.name);
        if statement.if_exists && view.is_none() {
            return Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP VIEW".to_string(),
            });
        }

        let Some(_) = view else {
            return Err(QueryError::General(format!(
                "view '{}' does not exist",
                statement.name
            )));
        };

        cassie
            .midge
            .defer_drop_view(&statement.name, cassie.runtime.schema_epoch())
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie.catalog.unregister_view(&statement.name);

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: "DROP VIEW".to_string(),
        })
    })())
}

fn execute_drop_table_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::DropTableStatement,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        if statement.if_exists && !cassie.catalog.exists(&statement.table) {
            return Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP TABLE".to_string(),
            });
        }

        cassie
            .midge
            .defer_drop_collection(&statement.table, cassie.runtime.schema_epoch())
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie.catalog.unregister_collection(&statement.table);

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: "DROP TABLE".to_string(),
        })
    })())
}

fn execute_alter_table_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::AlterTableStatement,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        let is_column_store = cassie
            .catalog
            .collection_storage_mode(&statement.table)
            .is_some_and(
                crate::catalog::collections::CollectionStorageMode::uses_column_store_storage,
            );
        execute_alter_table_operation(cassie, statement, is_column_store)?;

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: "ALTER TABLE".to_string(),
        })
    })())
}

fn execute_alter_table_operation(
    cassie: &Cassie,
    statement: &crate::sql::ast::AlterTableStatement,
    is_column_store: bool,
) -> Result<(), QueryError> {
    match &statement.operation {
        crate::sql::ast::AlterTableOperation::AddColumn { field, data_type } => {
            alter_table_add_column(cassie, &statement.table, field, data_type, is_column_store)
        }
        crate::sql::ast::AlterTableOperation::AddConstraint { constraints } => {
            alter_table_add_constraint(cassie, &statement.table, constraints)
        }
        crate::sql::ast::AlterTableOperation::DropColumn { field } => {
            alter_table_drop_column(cassie, &statement.table, field, is_column_store)
        }
        crate::sql::ast::AlterTableOperation::RenameColumn { from, to } => {
            alter_table_rename_column(cassie, &statement.table, from, to, is_column_store)
        }
        crate::sql::ast::AlterTableOperation::RenameTo { table } => {
            alter_table_rename_table(cassie, &statement.table, table)
        }
        crate::sql::ast::AlterTableOperation::AlterColumnSetDefault {
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
        crate::sql::ast::AlterTableOperation::AlterColumnDropDefault { field } => {
            super::sequence_command::alter_column_drop_default(cassie, &statement.table, field)
        }
        crate::sql::ast::AlterTableOperation::AlterColumnSetNotNull { field } => {
            super::sequence_command::alter_column_set_not_null(cassie, &statement.table, field)
        }
        crate::sql::ast::AlterTableOperation::AlterColumnDropNotNull { field } => {
            super::sequence_command::alter_column_drop_not_null(cassie, &statement.table, field)
        }
    }
}

fn alter_table_add_column(
    cassie: &Cassie,
    table: &str,
    field: &str,
    data_type: &crate::types::DataType,
    is_column_store: bool,
) -> Result<(), QueryError> {
    ensure_row_store_alter_supported(is_column_store, "ALTER TABLE ADD COLUMN")?;
    let field = FieldSchema {
        name: field.to_string(),
        data_type: data_type.clone(),
        nullable: true,
    };
    cassie
        .midge
        .alter_collection_add_column(table, field.clone())
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie
        .catalog
        .add_collection_field(table, field.name, field.data_type.clone());
    refresh_table_cardinality_stats(cassie, table)
}

fn alter_table_add_constraint(
    cassie: &Cassie,
    table: &str,
    constraints: &[crate::catalog::FieldConstraint],
) -> Result<(), QueryError> {
    let mut merged = cassie.catalog.get_constraints(table);
    crate::catalog::merge_constraint_set(&mut merged, constraints.to_vec());
    cassie
        .midge
        .save_constraints(table, merged.as_slice())
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_constraints(table, merged);
    Ok(())
}

fn alter_table_drop_column(
    cassie: &Cassie,
    table: &str,
    field: &str,
    is_column_store: bool,
) -> Result<(), QueryError> {
    ensure_row_store_alter_supported(is_column_store, "ALTER TABLE DROP COLUMN")?;
    cassie
        .midge
        .alter_collection_drop_column(table, field)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.remove_collection_field(table, field);
    refresh_table_cardinality_stats(cassie, table)
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
    cassie.catalog.rename_collection(table, next_table);
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

fn refresh_table_cardinality_stats(cassie: &Cassie, table: &str) -> Result<(), QueryError> {
    cassie
        .refresh_cardinality_stats(table)
        .map_err(|error| QueryError::General(error.to_string()))
}

fn execute_create_schema_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateSchemaStatement,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        if statement.if_not_exists && cassie.catalog.namespace_exists(&statement.schema) {
            return Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE SCHEMA".to_string(),
            });
        }

        cassie
            .midge
            .create_namespace(&statement.schema)
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie.catalog.register_namespace(&statement.schema, None);

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: "CREATE SCHEMA".to_string(),
        })
    })())
}

fn execute_drop_schema_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::DropSchemaStatement,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        if statement.if_exists && !cassie.catalog.namespace_exists(&statement.schema) {
            return Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP SCHEMA".to_string(),
            });
        }

        cassie
            .midge
            .drop_namespace(&statement.schema)
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie.catalog.unregister_namespace(&statement.schema);

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: "DROP SCHEMA".to_string(),
        })
    })())
}

fn execute_alter_schema_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::AlterSchemaStatement,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        let next_schema = match &statement.operation {
            crate::sql::ast::AlterSchemaOperation::RenameTo { schema } => schema.clone(),
        };
        let target_schema = statement.schema.clone();

        if cassie.catalog.namespace_exists(&next_schema) {
            return Err(QueryError::General(format!(
                "namespace '{next_schema}' already exists"
            )));
        }

        cassie
            .midge
            .rename_namespace(&target_schema, &next_schema)
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie
            .catalog
            .rename_namespace(&target_schema, &next_schema);

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: "ALTER SCHEMA".to_string(),
        })
    })())
}

fn execute_create_role_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateRoleStatement,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        cassie
            .create_role(
                &statement.name,
                statement.login,
                statement.password.clone(),
                statement.if_not_exists,
            )
            .map_err(|error| QueryError::General(error.to_string()))?;

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: "CREATE ROLE".to_string(),
        })
    })())
}

fn execute_alter_role_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::AlterRoleStatement,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        cassie
            .alter_role(&statement.name, statement.login, statement.password.clone())
            .map_err(|error| QueryError::General(error.to_string()))?;

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: "ALTER ROLE".to_string(),
        })
    })())
}

fn execute_drop_role_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::DropRoleStatement,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        cassie
            .drop_role(&statement.name, statement.if_exists)
            .map_err(|error| QueryError::General(error.to_string()))?;

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: "DROP ROLE".to_string(),
        })
    })())
}

fn execute_create_index_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateIndexStatement,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        let is_column_store = cassie
            .catalog
            .collection_storage_mode(&statement.table)
            .is_some_and(
                crate::catalog::collections::CollectionStorageMode::uses_column_store_storage,
            );
        if is_column_store && matches!(statement.kind, catalog::IndexKind::Column) {
            return Err(QueryError::General(
                "column indexes are not supported on column-store tables".to_string(),
            ));
        }
        if matches!(statement.kind, catalog::IndexKind::Vector) {
            let vector_index =
                super::vector_index_command::vector_index_metadata(cassie, statement)?;

            cassie
                .midge
                .put_vector_index(vector_index.clone())
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.register_vector_index(vector_index);
        }

        let metadata = catalog::IndexMeta {
            collection: statement.table.clone(),
            name: statement.name.clone(),
            field: statement.fields.first().cloned().unwrap_or_default(),
            fields: statement.fields.clone(),
            expressions: statement
                .expressions
                .iter()
                .filter_map(|expression| serde_json::to_string(expression).ok())
                .collect(),
            include_fields: statement.include_fields.clone(),
            predicate: statement
                .predicate
                .as_ref()
                .and_then(|predicate| serde_json::to_string(predicate).ok()),
            kind: statement.kind.clone(),
            unique: statement.unique,
            options: statement.options.clone(),
        };

        cassie
            .midge
            .put_index(&metadata)
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie.catalog.register_index(metadata.clone());
        if matches!(metadata.kind, catalog::IndexKind::Column) {
            cassie
                .midge
                .rebuild_column_batches_for_index(&metadata)
                .map_err(|error| QueryError::General(error.to_string()))?;
        }
        cassie
            .refresh_cardinality_stats(&statement.table)
            .map_err(|error| QueryError::General(error.to_string()))?;

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: "CREATE INDEX".to_string(),
        })
    })())
}

fn execute_drop_index_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::DropIndexStatement,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        let index = cassie.catalog.get_index(&statement.table, &statement.name);

        if statement.if_exists && index.is_none() {
            return Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP INDEX".to_string(),
            });
        }

        if let Some(index) = index.as_ref() {
            if matches!(index.kind, catalog::IndexKind::Vector) {
                cassie
                    .catalog
                    .unregister_vector_index(&statement.table, &index.field);
            }
        }

        cassie
            .midge
            .defer_drop_index(
                &statement.table,
                &statement.name,
                cassie.runtime.schema_epoch(),
            )
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie
            .catalog
            .unregister_index(&statement.table, &statement.name);
        cassie
            .refresh_cardinality_stats(&statement.table)
            .map_err(|error| QueryError::General(error.to_string()))?;

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: "DROP INDEX".to_string(),
        })
    })())
}

fn execute_create_function_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateFunctionStatement,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        if statement.if_not_exists && cassie.catalog.get_function(&statement.name).is_some() {
            return Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE FUNCTION".to_string(),
            });
        }

        let metadata = FunctionMeta {
            name: statement.name.clone(),
            args: statement
                .args
                .iter()
                .map(|arg| catalog::FunctionArgMeta {
                    name: arg.name.clone(),
                    data_type: arg.data_type.clone(),
                })
                .collect(),
            return_type: statement.return_type.clone(),
            volatility: Volatility::from(statement.volatility.clone()),
            body: statement.body.clone(),
        };

        cassie
            .midge
            .put_function(&metadata)
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie.catalog.register_function(metadata);

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: "CREATE FUNCTION".to_string(),
        })
    })())
}

fn execute_drop_function_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::DropFunctionStatement,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        if statement.if_exists && cassie.catalog.get_function(&statement.name).is_none() {
            return Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP FUNCTION".to_string(),
            });
        }

        cassie
            .midge
            .delete_function(&statement.name)
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie.catalog.unregister_function(&statement.name);

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: "DROP FUNCTION".to_string(),
        })
    })())
}

fn execute_create_procedure_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateProcedureStatement,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        if statement.if_not_exists && cassie.catalog.get_procedure(&statement.name).is_some() {
            return Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE PROCEDURE".to_string(),
            });
        }

        let metadata = ProcedureMeta {
            name: statement.name.clone(),
            args: statement
                .args
                .iter()
                .map(|arg| catalog::FunctionArgMeta {
                    name: arg.name.clone(),
                    data_type: arg.data_type.clone(),
                })
                .collect(),
            body: statement.body.clone(),
        };

        cassie
            .midge
            .put_procedure(&metadata)
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie.catalog.register_procedure(metadata);

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: "CREATE PROCEDURE".to_string(),
        })
    })())
}

fn execute_drop_procedure_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::DropProcedureStatement,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        if statement.if_exists && cassie.catalog.get_procedure(&statement.name).is_none() {
            return Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP PROCEDURE".to_string(),
            });
        }

        cassie
            .midge
            .delete_procedure(&statement.name)
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie.catalog.unregister_procedure(&statement.name);

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: "DROP PROCEDURE".to_string(),
        })
    })())
}

fn execute_call_procedure_command(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::CallProcedureStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> CommandExecution {
    CommandExecution::new((|| {
        let Some(metadata) = cassie.catalog.get_procedure(&statement.name) else {
            return Err(QueryError::General(format!(
                "procedure '{}' does not exist",
                statement.name
            )));
        };

        let call_session = session
            .cloned()
            .unwrap_or_else(|| CassieSession::new("postgres".to_string(), None));
        let empty_row = Vec::<(String, Value)>::new();
        let evaluated_args = statement
            .args
            .iter()
            .map(|expr| {
                filter::evaluate_expr_value(
                    &empty_row,
                    expr,
                    params,
                    None,
                    user_functions,
                    Some(&call_session),
                    None,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        call_session
            .enter_procedure_call(&statement.name)
            .map_err(|error| QueryError::General(error.to_string()))?;
        let body_result = cassie.execute_sql_with_controls(
            &call_session,
            &metadata.body,
            evaluated_args,
            crate::runtime::ExecutionMode::SimpleQuery,
            controls,
        );
        call_session.leave_procedure_call();
        body_result.map_err(|error| QueryError::General(error.to_string()))?;

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: "CALL".to_string(),
        })
    })())
}

fn execute_insert_command(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::InsertStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> CommandExecution {
    CommandExecution::new((|| {
        super::materialized_projection::reject_write(cassie, &statement.table)?;
        let result = super::dml::execute_insert(
            cassie,
            session,
            statement,
            params,
            user_functions,
            controls,
        )?;
        apply_write_side_effects(cassie, session, &statement.table, user_functions, controls)?;
        Ok(result)
    })())
}

fn execute_update_command(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::UpdateStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> CommandExecution {
    CommandExecution::new((|| {
        super::materialized_projection::reject_write(cassie, &statement.table)?;
        let result = super::dml::execute_update(
            cassie,
            session,
            statement,
            params,
            user_functions,
            controls,
        )?;
        apply_write_side_effects(cassie, session, &statement.table, user_functions, controls)?;
        Ok(result)
    })())
}

fn execute_delete_command(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::DeleteStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> CommandExecution {
    CommandExecution::new((|| {
        super::materialized_projection::reject_write(cassie, &statement.table)?;
        let result = super::dml::execute_delete(
            cassie,
            session,
            statement,
            params,
            user_functions,
            controls,
        )?;
        apply_write_side_effects(cassie, session, &statement.table, user_functions, controls)?;
        Ok(result)
    })())
}

fn apply_write_side_effects(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    if session.is_none_or(|session| !session.is_transaction_active()) {
        super::rollups::refresh_rollups_for_source(cassie, table, user_functions, controls)?;
    } else {
        super::rollups::mark_source_rollups_stale(cassie, table)?;
    }
    super::materialized_projection::mark_source_projections_stale(cassie, table)
}
