use super::*;

pub(super) fn execute_command(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    command: &LogicalCommand,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    check_timeout(controls)?;
    let mut invalidate_plan_cache = false;
    let result = match command {
        LogicalCommand::Show(statement) => {
            let variable = statement.variable.trim().to_ascii_lowercase();
            if variable.is_empty() {
                return Err(QueryError::General("SHOW requires a variable".to_string()));
            }

            match variable.as_str() {
                "search_path" => Ok(QueryResult {
                    columns: vec![ColumnMeta::text("search_path")],
                    rows: vec![vec![Value::String("public".to_string())]],
                    command: "SHOW".to_string(),
                }),
                "server_version" => Ok(QueryResult {
                    columns: vec![ColumnMeta::text("server_version")],
                    rows: vec![vec![Value::String(env!("CARGO_PKG_VERSION").to_string())]],
                    command: "SHOW".to_string(),
                }),
                _ => Err(QueryError::General(format!(
                    "unsupported SHOW variable '{}'",
                    statement.variable
                ))),
            }
        }
        LogicalCommand::Set(statement) => {
            let variable = statement.variable.trim().to_ascii_lowercase();
            if variable.is_empty() {
                return Err(QueryError::General("SET requires a variable".to_string()));
            }

            match variable.as_str() {
                "search_path" => {
                    let value = statement.value.as_deref().unwrap_or("").trim();
                    if value.is_empty() || value.eq_ignore_ascii_case("public") {
                        Ok(QueryResult {
                            columns: Vec::new(),
                            rows: Vec::new(),
                            command: "SET".to_string(),
                        })
                    } else {
                        Err(QueryError::General(format!(
                            "unsupported search_path value '{}' for SET",
                            value
                        )))
                    }
                }
                _ => Err(QueryError::General(format!(
                    "unsupported SET variable '{}', supported variables: search_path",
                    statement.variable
                ))),
            }
        }
        LogicalCommand::Insert(statement) => {
            super::materialized_projection::reject_write(cassie, &statement.table)?;
            let result = super::dml::execute_insert(
                cassie,
                session,
                statement,
                params,
                user_functions,
                controls,
            )?;
            if session
                .map(|session| !session.is_transaction_active())
                .unwrap_or(true)
            {
                super::rollups::refresh_rollups_for_source(
                    cassie,
                    &statement.table,
                    user_functions,
                    controls,
                )?;
            } else {
                super::rollups::mark_source_rollups_stale(cassie, &statement.table)?;
            }
            super::materialized_projection::mark_source_projections_stale(
                cassie,
                &statement.table,
            )?;
            Ok(result)
        }
        LogicalCommand::Update(statement) => {
            super::materialized_projection::reject_write(cassie, &statement.table)?;
            let result = super::dml::execute_update(
                cassie,
                session,
                statement,
                params,
                user_functions,
                controls,
            )?;
            if session
                .map(|session| !session.is_transaction_active())
                .unwrap_or(true)
            {
                super::rollups::refresh_rollups_for_source(
                    cassie,
                    &statement.table,
                    user_functions,
                    controls,
                )?;
            } else {
                super::rollups::mark_source_rollups_stale(cassie, &statement.table)?;
            }
            super::materialized_projection::mark_source_projections_stale(
                cassie,
                &statement.table,
            )?;
            Ok(result)
        }
        LogicalCommand::Delete(statement) => {
            super::materialized_projection::reject_write(cassie, &statement.table)?;
            let result = super::dml::execute_delete(
                cassie,
                session,
                statement,
                params,
                user_functions,
                controls,
            )?;
            if session
                .map(|session| !session.is_transaction_active())
                .unwrap_or(true)
            {
                super::rollups::refresh_rollups_for_source(
                    cassie,
                    &statement.table,
                    user_functions,
                    controls,
                )?;
            } else {
                super::rollups::mark_source_rollups_stale(cassie, &statement.table)?;
            }
            super::materialized_projection::mark_source_projections_stale(
                cassie,
                &statement.table,
            )?;
            Ok(result)
        }
        LogicalCommand::CreateRollup(statement) => {
            invalidate_plan_cache = true;
            super::rollups::create_rollup(cassie, statement, user_functions, controls)
        }
        LogicalCommand::RefreshRollup(statement) => {
            invalidate_plan_cache = true;
            super::rollups::refresh_rollup(cassie, &statement.name, user_functions, controls)
        }
        LogicalCommand::DropRollup(statement) => {
            invalidate_plan_cache = true;
            super::rollups::drop_rollup(cassie, &statement.name, statement.if_exists)
        }
        LogicalCommand::CreateMaterializedProjection(statement) => {
            invalidate_plan_cache = true;
            super::materialized_projection::create_materialized_projection(
                cassie,
                statement,
                user_functions,
                controls,
            )
        }
        LogicalCommand::RefreshMaterializedProjection(statement) => {
            invalidate_plan_cache = true;
            super::materialized_projection::refresh_materialized_projection(
                cassie,
                &statement.name,
                user_functions,
                controls,
            )
        }
        LogicalCommand::DropMaterializedProjection(statement) => {
            invalidate_plan_cache = true;
            super::materialized_projection::drop_materialized_projection(
                cassie,
                &statement.name,
                statement.if_exists,
            )
        }
        LogicalCommand::AlterMaterializedProjection(statement) => {
            invalidate_plan_cache = true;
            super::materialized_projection::alter_materialized_projection(
                cassie,
                statement,
                user_functions,
                controls,
            )
        }
        LogicalCommand::DropMaterializedProjectionVersion(statement) => {
            invalidate_plan_cache = true;
            super::materialized_projection::drop_materialized_projection_version(
                cassie,
                &statement.name,
                &statement.version_id,
            )
        }
        LogicalCommand::CreateRetentionPolicy(statement) => {
            invalidate_plan_cache = true;
            super::retention::create_retention_policy(cassie, statement)
        }
        LogicalCommand::AlterRetentionPolicy(statement) => {
            invalidate_plan_cache = true;
            super::retention::alter_retention_policy(cassie, statement)
        }
        LogicalCommand::DropRetentionPolicy(statement) => {
            invalidate_plan_cache = true;
            super::retention::drop_retention_policy(cassie, &statement.name, statement.if_exists)
        }
        LogicalCommand::EnforceRetentionPolicy(statement) => {
            super::retention::enforce_retention_policy(cassie, statement, user_functions, controls)
        }
        LogicalCommand::CreateTable(statement) => {
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

            cassie
                .midge
                .create_collection(&statement.table, schema.clone())
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
                    .put_index(index.clone())
                    .map_err(|error| QueryError::General(error.to_string()))?;
            }
            cassie.catalog.register_collection_with_constraints(
                &statement.table,
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
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE TABLE".to_string(),
            })
        }
        LogicalCommand::CreateView(statement) => {
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
            let metadata = crate::catalog::ViewMeta::new(
                statement.name.clone(),
                statement.query.clone(),
                schema,
            );

            cassie
                .midge
                .put_view(metadata.clone())
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.register_view(metadata);
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE VIEW".to_string(),
            })
        }
        LogicalCommand::DropView(statement) => {
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
                .delete_view(&statement.name)
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.unregister_view(&statement.name);
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP VIEW".to_string(),
            })
        }
        LogicalCommand::DropTable(statement) => {
            if statement.if_exists && !cassie.catalog.exists(&statement.table) {
                return Ok(QueryResult {
                    columns: Vec::new(),
                    rows: Vec::new(),
                    command: "DROP TABLE".to_string(),
                });
            }

            cassie
                .midge
                .drop_collection(&statement.table)
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.unregister_collection(&statement.table);
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP TABLE".to_string(),
            })
        }
        LogicalCommand::AlterTable(statement) => {
            match &statement.operation {
                crate::sql::ast::AlterTableOperation::AddColumn { field, data_type } => {
                    let field = FieldSchema {
                        name: field.clone(),
                        data_type: data_type.clone(),
                        nullable: true,
                    };
                    cassie
                        .midge
                        .alter_collection_add_column(&statement.table, field.clone())
                        .map_err(|error| QueryError::General(error.to_string()))?;
                    cassie.catalog.add_collection_field(
                        &statement.table,
                        field.name,
                        field.data_type.clone(),
                    );
                    cassie
                        .refresh_cardinality_stats(&statement.table)
                        .map_err(|error| QueryError::General(error.to_string()))?;
                    invalidate_plan_cache = true;
                }
                crate::sql::ast::AlterTableOperation::DropColumn { field } => {
                    cassie
                        .midge
                        .alter_collection_drop_column(&statement.table, field)
                        .map_err(|error| QueryError::General(error.to_string()))?;
                    cassie
                        .catalog
                        .remove_collection_field(&statement.table, field);
                    cassie
                        .refresh_cardinality_stats(&statement.table)
                        .map_err(|error| QueryError::General(error.to_string()))?;
                    invalidate_plan_cache = true;
                }
                crate::sql::ast::AlterTableOperation::RenameColumn { from, to } => {
                    cassie
                        .midge
                        .alter_collection_rename_column(&statement.table, from, to)
                        .map_err(|error| QueryError::General(error.to_string()))?;
                    cassie
                        .catalog
                        .rename_collection_field(&statement.table, from, to);
                    cassie
                        .refresh_cardinality_stats(&statement.table)
                        .map_err(|error| QueryError::General(error.to_string()))?;
                    invalidate_plan_cache = true;
                }
                crate::sql::ast::AlterTableOperation::RenameTo { table } => {
                    if cassie.catalog.exists(table) {
                        return Err(QueryError::General(format!(
                            "collection '{table}' already exists"
                        )));
                    }

                    cassie
                        .midge
                        .rename_collection(&statement.table, table)
                        .map_err(|error| QueryError::General(error.to_string()))?;
                    cassie.catalog.rename_collection(&statement.table, table);
                    invalidate_plan_cache = true;
                }
            }

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "ALTER TABLE".to_string(),
            })
        }
        LogicalCommand::CreateSchema(statement) => {
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
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE SCHEMA".to_string(),
            })
        }
        LogicalCommand::DropSchema(statement) => {
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
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP SCHEMA".to_string(),
            })
        }
        LogicalCommand::AlterSchema(statement) => {
            let next_schema = match &statement.operation {
                crate::sql::ast::AlterSchemaOperation::RenameTo { schema } => schema.clone(),
            };
            let target_schema = statement.schema.clone();

            if cassie.catalog.namespace_exists(&next_schema) {
                return Err(QueryError::General(format!(
                    "namespace '{next_schema}' already exists"
                )));
            };

            cassie
                .midge
                .rename_namespace(&target_schema, &next_schema)
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie
                .catalog
                .rename_namespace(&target_schema, &next_schema);
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "ALTER SCHEMA".to_string(),
            })
        }
        LogicalCommand::CreateRole(statement) => {
            cassie
                .create_role(
                    &statement.name,
                    statement.login,
                    statement.password.clone(),
                    statement.if_not_exists,
                )
                .map_err(|error| QueryError::General(error.to_string()))?;
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE ROLE".to_string(),
            })
        }
        LogicalCommand::AlterRole(statement) => {
            cassie
                .alter_role(&statement.name, statement.login, statement.password.clone())
                .map_err(|error| QueryError::General(error.to_string()))?;
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "ALTER ROLE".to_string(),
            })
        }
        LogicalCommand::DropRole(statement) => {
            cassie
                .drop_role(&statement.name, statement.if_exists)
                .map_err(|error| QueryError::General(error.to_string()))?;
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP ROLE".to_string(),
            })
        }
        LogicalCommand::CreateIndex(statement) => {
            if matches!(statement.kind, catalog::IndexKind::Vector) {
                let vector_index = vector_index_metadata(cassie, statement)?;

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
                .put_index(metadata.clone())
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
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE INDEX".to_string(),
            })
        }
        LogicalCommand::DropIndex(statement) => {
            let index = cassie.catalog.get_index(&statement.table, &statement.name);

            if statement.if_exists && index.is_none() {
                return Ok(QueryResult {
                    columns: Vec::new(),
                    rows: Vec::new(),
                    command: "DROP INDEX".to_string(),
                });
            }

            if let Some(index) = index {
                if matches!(index.kind, catalog::IndexKind::Vector) {
                    cassie
                        .midge
                        .delete_vector_index(&statement.table, &index.field)
                        .map_err(|error| QueryError::General(error.to_string()))?;
                    cassie
                        .catalog
                        .unregister_vector_index(&statement.table, &index.field);
                }
                if matches!(index.kind, catalog::IndexKind::Column) {
                    cassie
                        .midge
                        .delete_column_batches(&statement.table, &index.name)
                        .map_err(|error| QueryError::General(error.to_string()))?;
                }
            }

            cassie
                .midge
                .delete_index(&statement.table, &statement.name)
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie
                .catalog
                .unregister_index(&statement.table, &statement.name);
            cassie
                .refresh_cardinality_stats(&statement.table)
                .map_err(|error| QueryError::General(error.to_string()))?;
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP INDEX".to_string(),
            })
        }
        LogicalCommand::CreateFunction(statement) => {
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
                .put_function(metadata.clone())
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.register_function(metadata);
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE FUNCTION".to_string(),
            })
        }
        LogicalCommand::DropFunction(statement) => {
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
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP FUNCTION".to_string(),
            })
        }
        LogicalCommand::CreateProcedure(statement) => {
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
                .put_procedure(metadata.clone())
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.register_procedure(metadata);
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "CREATE PROCEDURE".to_string(),
            })
        }
        LogicalCommand::DropProcedure(statement) => {
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
            invalidate_plan_cache = true;

            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                command: "DROP PROCEDURE".to_string(),
            })
        }
        LogicalCommand::CallProcedure(statement) => {
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
        }
    };

    if invalidate_plan_cache {
        cassie
            .bump_schema_epoch_and_invalidate_query_cache()
            .map_err(|error| QueryError::General(error.to_string()))?;
    }

    result
}

fn vector_index_metadata(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateIndexStatement,
) -> Result<VectorIndexRecord, QueryError> {
    let schema = cassie
        .midge
        .collection_schema(&statement.table)
        .ok_or_else(|| {
            QueryError::General(format!(
                "collection '{}' not found while creating vector index",
                statement.table
            ))
        })?;

    let vector_field = schema
        .fields
        .iter()
        .find(|field| {
            statement
                .fields
                .first()
                .is_some_and(|value| field.name == *value)
        })
        .ok_or_else(|| {
            let field = statement.fields.first().cloned().unwrap_or_default();
            QueryError::General(format!(
                "index field '{}' does not exist in collection '{}'",
                field, statement.table
            ))
        })?;

    let dimensions = match vector_field.data_type {
        DataType::Vector(dimensions) => dimensions,
        _ => {
            return Err(QueryError::General(format!(
                "field '{}' is not a vector field",
                vector_field.name
            )));
        }
    };
    if cassie.embedding_provider.dimensions() != dimensions {
        return Err(QueryError::General(format!(
            "embedding dimension mismatch: field '{}' has {}, active provider '{}' model '{}' has {}",
            vector_field.name,
            dimensions,
            cassie.embedding_provider.provider_name(),
            cassie.embedding_provider.model_name(),
            cassie.embedding_provider.dimensions()
        )));
    }

    let source_field = statement
        .options
        .get("source_field")
        .ok_or_else(|| {
            QueryError::General("CREATE INDEX USING vector requires source_field".to_string())
        })?
        .to_string();

    let source_metadata = schema
        .fields
        .iter()
        .find(|field| field.name == source_field)
        .ok_or_else(|| {
            QueryError::General(format!(
                "source field '{}' does not exist in collection '{}'",
                source_field, statement.table
            ))
        })?;

    if !matches!(source_metadata.data_type, DataType::Text | DataType::Json) {
        return Err(QueryError::General(format!(
            "source field '{}' must be text/json for vector index",
            source_field
        )));
    }

    let index_type = match statement
        .options
        .get("index_type")
        .map(String::as_str)
        .unwrap_or("bruteforce")
    {
        "hnsw" => VectorIndexType::Hnsw,
        _ => VectorIndexType::BruteForce,
    };
    let hnsw = if index_type == VectorIndexType::Hnsw {
        Some(HnswIndexOptions {
            version: 1,
            m: statement
                .options
                .get("m")
                .and_then(|value| value.parse().ok())
                .unwrap_or(16),
            ef_construction: statement
                .options
                .get("ef_construction")
                .and_then(|value| value.parse().ok())
                .unwrap_or(64),
            ef_search: statement
                .options
                .get("ef_search")
                .and_then(|value| value.parse().ok())
                .unwrap_or(40),
        })
    } else {
        None
    };

    let metadata = VectorIndexMetadata {
        provider: cassie.embedding_provider.provider_name().to_string(),
        model: cassie.embedding_provider.model_name().to_string(),
        dimensions,
        metric: statement
            .options
            .get("metric")
            .and_then(|metric| metric.parse::<DistanceMetric>().ok())
            .unwrap_or(DistanceMetric::Cosine),
        index_type,
        hnsw,
    };

    Ok(VectorIndexRecord {
        collection: statement.table.clone(),
        field: statement.fields.first().cloned().unwrap_or_default(),
        source_field,
        metadata,
    })
}
