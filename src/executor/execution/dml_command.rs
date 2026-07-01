use super::{
    catalog, check_timeout, filter, Cassie, CassieSession, FunctionMeta, HashMap, LogicalCommand,
    ProcedureMeta, QueryError, QueryExecutionControls, QueryResult, Value, Volatility,
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
        LogicalCommand::CreateTable(statement) => Some(CommandExecution::invalidating(
            super::schema_command::create_table(cassie, statement),
        )),
        LogicalCommand::CreateGraph(statement) => Some(CommandExecution::invalidating(
            super::schema_command::create_graph(cassie, statement),
        )),
        LogicalCommand::CreateView(statement) => Some(CommandExecution::invalidating(
            super::schema_command::create_view(cassie, statement),
        )),
        LogicalCommand::DropView(statement) => Some(CommandExecution::invalidating(
            super::schema_command::drop_view(cassie, statement),
        )),
        LogicalCommand::DropTable(statement) => Some(CommandExecution::invalidating(
            super::schema_command::drop_table(cassie, statement),
        )),
        LogicalCommand::AlterTable(statement) => Some(CommandExecution::invalidating(
            super::schema_command::alter_table(cassie, statement),
        )),
        LogicalCommand::CreateSchema(statement) => Some(CommandExecution::invalidating(
            super::schema_command::create_schema(cassie, statement),
        )),
        LogicalCommand::DropSchema(statement) => Some(CommandExecution::invalidating(
            super::schema_command::drop_schema(cassie, statement),
        )),
        LogicalCommand::AlterSchema(statement) => Some(CommandExecution::invalidating(
            super::schema_command::alter_schema(cassie, statement),
        )),
        LogicalCommand::CreateRole(statement) => Some(CommandExecution::invalidating(
            super::schema_command::create_role(cassie, statement),
        )),
        LogicalCommand::AlterRole(statement) => Some(CommandExecution::invalidating(
            super::schema_command::alter_role(cassie, statement),
        )),
        LogicalCommand::DropRole(statement) => Some(CommandExecution::invalidating(
            super::schema_command::drop_role(cassie, statement),
        )),
        LogicalCommand::CreateIndex(statement) => Some(CommandExecution::invalidating(
            super::schema_command::create_index(cassie, statement),
        )),
        LogicalCommand::DropIndex(statement) => Some(CommandExecution::invalidating(
            super::schema_command::drop_index(cassie, statement),
        )),
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
