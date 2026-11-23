use super::{
    catalog, check_timeout, filter, Cassie, CassieSession, FunctionMeta, HashMap, LogicalCommand,
    ProcedureMeta, QueryError, QueryExecutionControls, QueryResult, Value, Volatility,
};

#[path = "dml_unique_wait.rs"]
mod unique_wait;
use unique_wait::execute_atomic_dml;

pub(super) fn execute_command(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    command: &LogicalCommand,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    execute_command_for_operation(
        cassie,
        session,
        command,
        params,
        user_functions,
        (controls, None),
    )
}

pub(super) fn execute_command_for_operation(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    command: &LogicalCommand,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    scope: (
        &QueryExecutionControls,
        Option<&crate::app::FailureAuthority>,
    ),
) -> Result<QueryResult, QueryError> {
    let (controls, authority) = scope;
    check_timeout(controls)?;
    let outcome = command_outcome(
        cassie,
        session,
        command,
        params,
        user_functions,
        (controls, authority),
    );

    if outcome.invalidate_plan_cache {
        let bumped = cassie.bump_schema_epoch_and_invalidate_query_cache();
        // A command that already failed (for example canceled) keeps its own
        // error; the invalidation failure must not replace it.
        if outcome.result.is_ok() {
            bumped.map_err(|error| QueryError::General(error.to_string()))?;
        }
    }

    outcome.result
}

fn command_outcome(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    command: &LogicalCommand,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    scope: (
        &QueryExecutionControls,
        Option<&crate::app::FailureAuthority>,
    ),
) -> CommandExecution {
    let (controls, authority) = scope;
    execute_session_or_dml_command(cassie, session, command, params, user_functions, controls)
        .or_else(|| {
            execute_projection_command_group(cassie, session, command, user_functions, controls)
        })
        .or_else(|| {
            execute_retention_sequence_group(cassie, session, command, user_functions, controls)
        })
        .or_else(|| execute_schema_object_group(cassie, session, command, controls))
        .or_else(|| {
            execute_routine_group(
                cassie,
                session,
                command,
                params,
                user_functions,
                (controls, authority),
            )
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
            super::session_command::execute_show(session, statement, controls),
        )),
        LogicalCommand::Set(statement) => Some(CommandExecution::new(
            super::session_command::execute_set(session, statement, controls),
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
        LogicalCommand::Merge(statement) => Some(execute_merge_command(
            cassie,
            session,
            statement,
            params,
            user_functions,
            controls,
        )),
        LogicalCommand::Truncate(statement) => Some(CommandExecution::new(
            super::dml::execute_truncate(cassie, session, statement, controls),
        )),
        _ => None,
    }
}

fn execute_projection_command_group(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    command: &LogicalCommand,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Option<CommandExecution> {
    if let Some(execution) =
        execute_materialized_view_command(cassie, session, command, user_functions, controls)
    {
        return Some(execution);
    }
    match command {
        LogicalCommand::CreateRollup(statement) => Some(CommandExecution::invalidating(
            super::rollups::create_rollup(cassie, statement, user_functions, controls),
        )),
        LogicalCommand::RefreshRollup(statement) => Some(CommandExecution::invalidating(
            super::rollups::refresh_rollup(cassie, &statement.name, user_functions, controls),
        )),
        LogicalCommand::DropRollup(statement) => Some(CommandExecution::invalidating(
            super::rollups::drop_rollup(cassie, &statement.name, statement.if_exists, controls),
        )),
        LogicalCommand::CreateMaterializedProjection(statement) => {
            Some(CommandExecution::invalidating(
                super::materialized_projection::create_materialized_projection(
                    cassie,
                    session,
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
                    session,
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
                    controls,
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
                super::materialized_projection_versions::drop_materialized_projection_version(
                    cassie,
                    &statement.name,
                    &statement.version_id,
                    controls,
                ),
            ))
        }
        LogicalCommand::VerifyProjection(statement) => Some(CommandExecution::new(
            super::materialized_projection::verify_projection(cassie, statement, controls),
        )),
        LogicalCommand::DiffProjection(statement) => Some(CommandExecution::new(
            super::projection_diff::diff_projection(cassie, statement, controls),
        )),
        LogicalCommand::CompareProjection(statement) => Some(CommandExecution::new(
            super::projection_diff::compare_projection(cassie, statement, controls),
        )),
        LogicalCommand::PlanRepairProjection(statement) => Some(CommandExecution::new(
            super::projection_repair::plan_repair_projection(
                cassie,
                &statement.target,
                statement.scope,
                controls,
            ),
        )),
        LogicalCommand::RepairProjection(statement) => Some(CommandExecution::new(
            super::projection_repair::repair_projection(cassie, statement, controls),
        )),
        _ => None,
    }
}

fn execute_materialized_view_command(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    command: &LogicalCommand,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Option<CommandExecution> {
    match command {
        LogicalCommand::CreateMaterializedView(statement) => Some(CommandExecution::invalidating(
            super::materialized_projection::create_materialized_view(
                cassie,
                session,
                statement,
                user_functions,
                controls,
            ),
        )),
        LogicalCommand::RefreshMaterializedView(statement) => Some(CommandExecution::invalidating(
            super::materialized_projection::refresh_materialized_view(
                cassie,
                session,
                statement,
                user_functions,
                controls,
            ),
        )),
        LogicalCommand::RenameMaterializedView(statement) => Some(CommandExecution::invalidating(
            super::materialized_view_rename::rename(cassie, session, statement, controls),
        )),
        LogicalCommand::DropMaterializedView(statement) => Some(CommandExecution::invalidating(
            super::materialized_projection::drop_materialized_view(cassie, statement, controls),
        )),
        _ => None,
    }
}

fn execute_retention_sequence_group(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    command: &LogicalCommand,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Option<CommandExecution> {
    match command {
        LogicalCommand::CreateRetentionPolicy(statement) => Some(CommandExecution::invalidating(
            super::retention::create_retention_policy(cassie, statement, controls),
        )),
        LogicalCommand::AlterRetentionPolicy(statement) => Some(CommandExecution::invalidating(
            super::retention::alter_retention_policy(cassie, session, statement, controls),
        )),
        LogicalCommand::DropRetentionPolicy(statement) => Some(CommandExecution::invalidating(
            super::retention::drop_retention_policy(
                cassie,
                &statement.name,
                statement.if_exists,
                controls,
            ),
        )),
        LogicalCommand::EnforceRetentionPolicy(statement) => Some(CommandExecution::new(
            super::retention::enforce_retention_policy(
                cassie,
                session,
                statement,
                user_functions,
                controls,
            ),
        )),
        LogicalCommand::CreateSequence(statement) => Some(CommandExecution::invalidating(
            super::sequence_command::create_sequence(cassie, statement, controls),
        )),
        LogicalCommand::AlterSequence(statement) => Some(CommandExecution::invalidating(
            super::sequence_command::alter_sequence(cassie, statement, controls),
        )),
        LogicalCommand::DropSequence(statement) => Some(CommandExecution::invalidating(
            super::sequence_command::drop_sequence(cassie, statement, controls),
        )),
        _ => None,
    }
}

fn execute_schema_object_group(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    command: &LogicalCommand,
    controls: &QueryExecutionControls,
) -> Option<CommandExecution> {
    match command {
        LogicalCommand::CommentOn(statement) => Some(CommandExecution::invalidating(
            super::comment_command::execute(cassie, session, statement, controls),
        )),
        LogicalCommand::CreateTable(statement) => Some(CommandExecution::creation(
            super::schema_creation::create_table(cassie, statement, controls),
        )),
        LogicalCommand::CreateDatabase(statement) => Some(CommandExecution::new(
            super::schema_command::create_database(cassie, statement, controls),
        )),
        LogicalCommand::DropDatabase(statement) => Some(CommandExecution::invalidating(
            super::schema_command::drop_database(cassie, session, statement, controls),
        )),
        LogicalCommand::CreateView(statement) => Some(CommandExecution::invalidating(
            super::schema_command::create_view(cassie, session, statement, controls),
        )),
        LogicalCommand::DropView(statement) => Some(CommandExecution::invalidating(
            super::schema_command::drop_view(cassie, statement, controls),
        )),
        LogicalCommand::DropTable(statement) => Some(CommandExecution::invalidating(
            super::schema_command::drop_table(cassie, session, statement, controls),
        )),
        LogicalCommand::AlterTable(statement) => Some(CommandExecution::invalidating(
            super::schema_command::alter_table(cassie, session, statement, controls),
        )),
        LogicalCommand::CreateSchema(statement) => Some(CommandExecution::invalidating(
            super::schema_command::create_schema(cassie, statement, controls),
        )),
        LogicalCommand::DropSchema(statement) => Some(CommandExecution::invalidating(
            super::schema_command::drop_schema(cassie, session, statement, controls),
        )),
        LogicalCommand::AlterSchema(statement) => Some(CommandExecution::invalidating(
            super::schema_command::alter_schema(cassie, session, statement, controls),
        )),
        LogicalCommand::Extension(statement) => Some(CommandExecution::invalidating(
            super::extension_command::execute(cassie, session, statement, controls),
        )),
        LogicalCommand::Type(statement) => Some(CommandExecution::invalidating(
            super::type_command::execute(cassie, session, statement, controls),
        )),
        LogicalCommand::PropertyGraph(statement) => Some(CommandExecution::invalidating(
            super::property_graph_command::execute(cassie, session, statement, controls),
        )),
        LogicalCommand::CreateRole(statement) => Some(CommandExecution::invalidating(
            super::role_command::create_role(cassie, session, statement, controls),
        )),
        LogicalCommand::AlterRole(statement) => Some(CommandExecution::invalidating(
            super::role_command::alter_role(cassie, statement, controls),
        )),
        LogicalCommand::DropRole(statement) => Some(CommandExecution::invalidating(
            super::role_command::drop_role(cassie, statement, controls),
        )),
        LogicalCommand::GrantDatabaseConnect(statement) => Some(CommandExecution::invalidating(
            super::role_command::grant_database_connect(cassie, session, statement, controls),
        )),
        LogicalCommand::RevokeDatabaseConnect(statement) => Some(CommandExecution::invalidating(
            super::role_command::revoke_database_connect(cassie, session, statement, controls),
        )),
        LogicalCommand::CreateIndex(statement) => Some(CommandExecution::creation(
            super::schema_creation::create_index(cassie, session, statement, controls),
        )),
        LogicalCommand::DropIndex(statement) => Some(CommandExecution::invalidating(
            super::schema_command::drop_index(cassie, statement, controls),
        )),
        LogicalCommand::AlterIndex(statement) => Some(CommandExecution::invalidating(
            super::schema_command::alter_index_storage_parameters(cassie, statement, controls),
        )),
        LogicalCommand::Reindex(statement) => Some(CommandExecution::invalidating(
            super::schema_command::reindex(cassie, statement, controls),
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
    scope: (
        &QueryExecutionControls,
        Option<&crate::app::FailureAuthority>,
    ),
) -> Option<CommandExecution> {
    let (controls, authority) = scope;
    match command {
        LogicalCommand::CreateFunction(statement) => {
            Some(execute_create_function_command(cassie, statement, controls))
        }
        LogicalCommand::DropFunction(statement) => {
            Some(execute_drop_function_command(cassie, statement, controls))
        }
        LogicalCommand::CreateProcedure(statement) => Some(execute_create_procedure_command(
            cassie, statement, controls,
        )),
        LogicalCommand::DropProcedure(statement) => {
            Some(execute_drop_procedure_command(cassie, statement, controls))
        }
        LogicalCommand::CallProcedure(statement) => Some(execute_call_procedure_command(
            cassie,
            session,
            statement,
            params,
            user_functions,
            (controls, authority),
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

    fn creation(result: Result<super::schema_creation::CreationOutcome, QueryError>) -> Self {
        match result {
            Ok(outcome) => Self {
                result: Ok(outcome.result),
                invalidate_plan_cache: outcome.created,
            },
            Err(error) => Self::new(Err(error)),
        }
    }
}

fn execute_create_function_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateFunctionStatement,
    controls: &QueryExecutionControls,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        if statement.if_not_exists && cassie.catalog.get_function(&statement.name).is_some() {
            return super::command_output::empty("CREATE FUNCTION", controls);
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
            object_id: routine_object_id(
                cassie,
                cassie
                    .catalog
                    .get_function(&statement.name)
                    .map(|existing| existing.object_id),
            )?,
        };

        let output = super::command_output::empty("CREATE FUNCTION", controls)?;
        cassie
            .midge
            .put_function(&metadata)
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie.catalog.register_function(metadata);

        Ok(output)
    })())
}

/// The identity of a routine being created: a replaced routine keeps its
/// identity, and a new or legacy (zero) one draws from the common counter.
fn routine_object_id(cassie: &Cassie, existing: Option<u64>) -> Result<u64, QueryError> {
    match existing {
        Some(object_id) if object_id != 0 => Ok(object_id),
        _ => cassie.midge.allocate_object_id().map_err(QueryError::from),
    }
}

fn execute_drop_function_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::DropFunctionStatement,
    controls: &QueryExecutionControls,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        if statement.if_exists && cassie.catalog.get_function(&statement.name).is_none() {
            return super::command_output::empty("DROP FUNCTION", controls);
        }
        if statement.cascade {
            return Err(QueryError::Cassie(
                crate::app::DiagnosticError::new("0A000", "DROP FUNCTION CASCADE is not supported")
                    .into(),
            ));
        }
        if let Some(function) = cassie.catalog.get_function(&statement.name) {
            super::view_dependents::reject_function_view_dependents(cassie, &function)?;
        }

        let output = super::command_output::empty("DROP FUNCTION", controls)?;
        cassie
            .midge
            .delete_function(&statement.name)
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie.catalog.unregister_function(&statement.name);

        Ok(output)
    })())
}

fn execute_create_procedure_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateProcedureStatement,
    controls: &QueryExecutionControls,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        if statement.if_not_exists && cassie.catalog.get_procedure(&statement.name).is_some() {
            return super::command_output::empty("CREATE PROCEDURE", controls);
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
            object_id: routine_object_id(
                cassie,
                cassie
                    .catalog
                    .get_procedure(&statement.name)
                    .map(|existing| existing.object_id),
            )?,
        };

        let output = super::command_output::empty("CREATE PROCEDURE", controls)?;
        cassie
            .midge
            .put_procedure(&metadata)
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie.catalog.register_procedure(metadata);

        Ok(output)
    })())
}

fn execute_drop_procedure_command(
    cassie: &Cassie,
    statement: &crate::sql::ast::DropProcedureStatement,
    controls: &QueryExecutionControls,
) -> CommandExecution {
    CommandExecution::invalidating((|| {
        if statement.if_exists && cassie.catalog.get_procedure(&statement.name).is_none() {
            return super::command_output::empty("DROP PROCEDURE", controls);
        }

        let output = super::command_output::empty("DROP PROCEDURE", controls)?;
        cassie
            .midge
            .delete_procedure(&statement.name)
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie.catalog.unregister_procedure(&statement.name);

        Ok(output)
    })())
}

fn execute_call_procedure_command(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::CallProcedureStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    scope: (
        &QueryExecutionControls,
        Option<&crate::app::FailureAuthority>,
    ),
) -> CommandExecution {
    let (controls, authority) = scope;
    CommandExecution::new((|| {
        let Some(metadata) = cassie.catalog.get_procedure(&statement.name) else {
            return Err(QueryError::General(format!(
                "procedure '{}' does not exist",
                statement.name
            )));
        };

        let call_session = session
            .cloned()
            .unwrap_or_else(|| CassieSession::new("root".to_string(), None));
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

        let output = super::command_output::empty("CALL", controls)?;
        call_session
            .enter_procedure_call(&statement.name)
            .map_err(|error| QueryError::General(error.to_string()))?;
        let body_result = cassie.execute_sql_with_controls_for_operation(
            &call_session,
            &metadata.body,
            evaluated_args,
            crate::runtime::ExecutionMode::SimpleQuery,
            (controls, authority),
        );
        call_session.leave_procedure_call();
        body_result.map_err(|error| QueryError::General(error.to_string()))?;

        Ok(output)
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
        let result = if let Some(arbiter) = super::dml_upsert_wait::arbiter(cassie, statement) {
            super::dml_upsert_wait::execute(super::dml_upsert_wait::UpsertRequest {
                cassie,
                session,
                statement,
                params,
                functions: user_functions,
                controls,
                arbiter: &arbiter,
            })?
        } else {
            execute_atomic_dml(
                cassie,
                session,
                &statement.table,
                controls,
                statement.on_conflict.is_none(),
                false,
                |working_cassie, working, controls| {
                    super::dml::execute_insert(
                        working_cassie,
                        Some(working),
                        statement,
                        params,
                        user_functions,
                        controls,
                    )
                },
            )?
        };
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
        super::dml::admit_read_committed_update_rows(
            cassie,
            session,
            statement,
            params,
            user_functions,
            controls,
        )?;
        let result = execute_atomic_dml(
            cassie,
            session,
            &statement.table,
            controls,
            false,
            true,
            |working_cassie, working, controls| {
                super::dml::execute_update(
                    working_cassie,
                    Some(working),
                    statement,
                    params,
                    user_functions,
                    controls,
                )
            },
        )?;
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
        let result = execute_atomic_dml(
            cassie,
            session,
            &statement.table,
            controls,
            false,
            true,
            |working_cassie, working, controls| {
                super::dml::execute_delete(
                    working_cassie,
                    Some(working),
                    statement,
                    params,
                    user_functions,
                    controls,
                )
            },
        )?;
        Ok(result)
    })())
}

fn execute_merge_command(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::MergeStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> CommandExecution {
    CommandExecution::new((|| {
        super::materialized_projection::reject_write(cassie, &statement.table)?;
        let result = execute_atomic_dml(
            cassie,
            session,
            &statement.table,
            controls,
            false,
            true,
            |working_cassie, working, controls| {
                super::dml::execute_merge(
                    working_cassie,
                    Some(working),
                    statement,
                    params,
                    user_functions,
                    controls,
                )
            },
        )?;
        Ok(result)
    })())
}
