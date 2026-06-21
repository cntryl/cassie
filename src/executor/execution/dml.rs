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
            execute_insert(cassie, session, statement, params, user_functions, controls)
        }
        LogicalCommand::Update(statement) => {
            execute_update(cassie, session, statement, params, user_functions, controls)
        }
        LogicalCommand::Delete(statement) => {
            execute_delete(cassie, session, statement, params, user_functions, controls)
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
            cassie.catalog.register_index(metadata);
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

fn execute_insert(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::InsertStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let schema = cassie.catalog.get_schema(&statement.table).ok_or_else(|| {
        QueryError::General(format!("collection '{}' not found", statement.table))
    })?;

    let source_rows =
        insert_source_rows(cassie, session, statement, params, user_functions, controls)?;
    let source_width = source_rows
        .first()
        .map(Vec::len)
        .unwrap_or_else(|| insert_source_width(statement, &schema));
    let target_fields = insert_target_fields(statement, &schema, source_width)?;
    for row in &source_rows {
        if row.len() != target_fields.len() {
            return Err(QueryError::General(format!(
                "INSERT column/value counts mismatch: {} columns, {} values",
                target_fields.len(),
                row.len()
            )));
        }
    }

    let inserted_count = source_rows.len();
    let mut returning_rows = Vec::new();
    for source_row in source_rows {
        let payload = payload_from_insert_row(&target_fields, &source_row);
        let row_id = cassie
            .write_document_for_session(
                session,
                &statement.table,
                None,
                serde_json::Value::Object(payload),
                true,
                None,
            )
            .map_err(|error| QueryError::General(error.to_string()))?;

        if !statement.returning.is_empty() {
            let document = cassie
                .get_document_for_session(session, &statement.table, &row_id)
                .map_err(|error| QueryError::General(error.to_string()))?
                .ok_or_else(|| {
                    QueryError::General(format!(
                        "inserted row '{row_id}' was not found in '{}'",
                        statement.table
                    ))
                })?;

            returning_rows.push(inserted_row_to_batch_row(
                &row_id,
                &schema,
                &document.payload,
            ));
        }
    }

    if statement.returning.is_empty() {
        return Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: format!("INSERT 0 {inserted_count}"),
        });
    }

    let projected = projection::project_rows(
        returning_rows,
        &statement.returning,
        params,
        None,
        user_functions,
        session,
    )?;

    let column_schema = cassie.catalog.get_schema(&statement.table);
    let columns =
        dml_returning_columns(&statement.returning, column_schema.as_ref(), user_functions);

    Ok(QueryResult {
        columns,
        rows: projected.into_iter().map(BatchRow::into_values).collect(),
        command: format!("INSERT 0 {inserted_count}"),
    })
}

fn insert_source_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::InsertStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Vec<Value>>, QueryError> {
    match &statement.source {
        InsertSource::Values(rows) => rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|expr| {
                        insert_expr_to_json(expr, params)
                            .map_err(QueryError::General)
                            .map(|value| json_to_value(&value))
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect::<Result<Vec<_>, _>>(),
        InsertSource::Select(select) => {
            let logical = LogicalPlan {
                command: None,
                source: select.source.clone(),
                collection: match &select.source {
                    QuerySource::Collection(name) | QuerySource::Cte(name) => name.clone(),
                    QuerySource::Subquery { alias, .. } => alias.clone(),
                    QuerySource::SingleRow => "single_row".to_string(),
                    QuerySource::Join { .. } => "join".to_string(),
                },
                ctes: select.ctes.clone(),
                distinct: select.distinct,
                distinct_on: select.distinct_on.clone(),
                projection: select.projection.clone(),
                filter: select.filter.clone(),
                group_by: select.group_by.clone(),
                having: select.having.clone(),
                order: select.order.clone(),
                limit: select.limit,
                offset: select.offset,
                set: select.set.clone(),
            };
            let mut cte_context = CteContext::new();
            let rows = execute_plan(
                cassie,
                session,
                &logical,
                &mut cte_context,
                user_functions,
                params,
                controls,
            )?;
            Ok(rows
                .into_iter()
                .map(|row| {
                    row.into_entries()
                        .into_iter()
                        .map(|(_, value)| value)
                        .collect()
                })
                .collect())
        }
    }
}

fn insert_source_width(
    statement: &crate::sql::ast::InsertStatement,
    schema: &CollectionSchema,
) -> usize {
    match &statement.source {
        InsertSource::Values(rows) => rows.first().map_or(0, Vec::len),
        InsertSource::Select(select) => {
            if matches!(
                select.projection.as_slice(),
                [crate::sql::ast::SelectItem::Wildcard]
            ) {
                schema.fields.len()
            } else {
                select.projection.len()
            }
        }
    }
}

fn payload_from_insert_row(
    target_fields: &[FieldMeta],
    source_row: &[Value],
) -> serde_json::Map<String, serde_json::Value> {
    let mut payload = serde_json::Map::with_capacity(target_fields.len());
    for (field, value) in target_fields.iter().zip(source_row.iter()) {
        payload.insert(field.name.clone(), value_to_json(value));
    }
    payload
}

fn insert_target_fields(
    statement: &crate::sql::ast::InsertStatement,
    schema: &CollectionSchema,
    value_count: usize,
) -> Result<Vec<FieldMeta>, QueryError> {
    if statement.columns.is_empty() {
        if schema.fields.len() != value_count {
            return Err(QueryError::General(format!(
                "INSERT column/value counts mismatch: {} columns, {} values",
                schema.fields.len(),
                value_count
            )));
        }

        return Ok(schema.fields.clone());
    }

    if statement.columns.len() != value_count {
        return Err(QueryError::General(format!(
            "INSERT column/value counts mismatch: {} columns, {} values",
            statement.columns.len(),
            value_count
        )));
    }

    statement
        .columns
        .iter()
        .map(|column| {
            schema
                .fields
                .iter()
                .find(|field| field.name.eq_ignore_ascii_case(column))
                .cloned()
                .ok_or_else(|| {
                    QueryError::General(format!(
                        "INSERT target column '{}' does not exist in '{}'",
                        column, statement.table
                    ))
                })
        })
        .collect()
}

fn insert_expr_to_json(expr: &Expr, params: &[Value]) -> Result<serde_json::Value, String> {
    match expr {
        Expr::StringLiteral(value) => Ok(serde_json::Value::String(value.clone())),
        Expr::NumberLiteral(value) => number_literal_to_json(*value),
        Expr::BoolLiteral(value) => Ok(serde_json::Value::Bool(*value)),
        Expr::Null => Ok(serde_json::Value::Null),
        Expr::Param(index) => params
            .get(*index)
            .map(value_to_json)
            .ok_or_else(|| format!("missing bind parameter ${}", index + 1)),
        Expr::Column(_)
        | Expr::Function(_)
        | Expr::IsNull { .. }
        | Expr::InList { .. }
        | Expr::Between { .. }
        | Expr::Not { .. }
        | Expr::Cast { .. }
        | Expr::Exists(_)
        | Expr::Binary {
            left: _,
            op: _,
            right: _,
        } => Err("INSERT VALUES only supports literals and bind parameters".to_string()),
    }
}

fn number_literal_to_json(value: f64) -> Result<serde_json::Value, String> {
    if !value.is_finite() {
        return Err("INSERT VALUES requires finite numeric literals".to_string());
    }

    if value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
        return Ok(serde_json::Value::Number((value as i64).into()));
    }

    serde_json::Number::from_f64(value)
        .map(serde_json::Value::Number)
        .ok_or_else(|| "INSERT VALUES requires finite numeric literals".to_string())
}

fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(value) => serde_json::Value::Bool(*value),
        Value::Int64(value) => serde_json::Value::Number((*value).into()),
        Value::Float64(value) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::String(value) => serde_json::Value::String(value.clone()),
        Value::Vector(value) => serde_json::Value::Array(
            value
                .values
                .iter()
                .filter_map(|value| serde_json::Number::from_f64((*value).into()))
                .map(serde_json::Value::Number)
                .collect(),
        ),
        Value::Json(value) => value.clone(),
    }
}

fn update_assignment_to_json(
    field: &str,
    value: &Value,
    schema: &CollectionSchema,
) -> serde_json::Value {
    if let Some(field_meta) = schema
        .fields
        .iter()
        .find(|candidate| candidate.name.eq_ignore_ascii_case(field))
    {
        if let DataType::Vector(dimensions) = &field_meta.data_type {
            if let Some(text) = value.as_str() {
                if let Some(vector) = super::scored::parse_vector_literal(text) {
                    if vector.len() == *dimensions {
                        return serde_json::Value::Array(
                            vector
                                .into_iter()
                                .map(|component| {
                                    serde_json::Number::from_f64(component as f64)
                                        .map(serde_json::Value::Number)
                                })
                                .collect::<Option<Vec<_>>>()
                                .unwrap_or_default(),
                        );
                    }
                }
            }
        }
        if matches!(
            field_meta.data_type,
            DataType::SmallInt | DataType::Int | DataType::BigInt
        ) {
            if let Value::Float64(number) = value {
                if number.is_finite()
                    && number.fract() == 0.0
                    && *number >= i64::MIN as f64
                    && *number <= i64::MAX as f64
                {
                    return serde_json::Value::Number((*number as i64).into());
                }
            }
        }
    }

    value_to_json(value)
}

fn inserted_row_to_batch_row(
    row_id: &str,
    schema: &CollectionSchema,
    payload: &serde_json::Value,
) -> BatchRow {
    let mut row = Vec::with_capacity(schema.fields.len() + 1);
    row.push(("_id".to_string(), Value::String(row_id.to_string())));

    for field in &schema.fields {
        let value = payload
            .get(&field.name)
            .map(json_to_value)
            .unwrap_or(Value::Null);
        row.push((field.name.clone(), value));
    }

    BatchRow::new(row)
}

fn dml_returning_columns(
    returning: &[SelectItem],
    schema: Option<&CollectionSchema>,
    user_functions: &HashMap<String, FunctionMeta>,
) -> Vec<ColumnMeta> {
    let mut columns = aggregate::columns_from_projection(returning, schema, user_functions);
    if returning
        .iter()
        .any(|item| matches!(item, SelectItem::Wildcard))
    {
        for column in &mut columns {
            if column.name == "id" {
                column.name = "_id".to_string();
                break;
            }
        }
    }
    columns
}

fn json_to_value(value: &serde_json::Value) -> Value {
    if value.is_null() {
        return Value::Null;
    }
    if let Some(value) = value.as_str() {
        return Value::String(value.to_string());
    }
    if let Some(value) = value.as_bool() {
        return Value::Bool(value);
    }
    if let Some(value) = value.as_i64() {
        return Value::Int64(value);
    }
    if let Some(value) = value.as_u64().and_then(|value| i64::try_from(value).ok()) {
        return Value::Int64(value);
    }
    if let Some(value) = value.as_f64() {
        return Value::Float64(value);
    }
    Value::Json(value.clone())
}

fn execute_update(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::UpdateStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    check_timeout(controls)?;
    let schema = cassie.catalog.get_schema(&statement.table).ok_or_else(|| {
        QueryError::General(format!("collection '{}' not found", statement.table))
    })?;

    let batches = scan::scan(cassie, session, &statement.table)?;
    ensure_temp_budget(controls, &batches)?;
    let rows = batch::flatten_batches(batches);
    let matched_rows = if let Some(filter_expr) = &statement.filter {
        filter::filter_rows(rows, filter_expr, params, None, user_functions, session)?
    } else {
        rows
    };

    let mut prepared_rows = Vec::with_capacity(matched_rows.len());
    for row in &matched_rows {
        let row_id = row_id_from_batch_row(row)?;
        let current = cassie
            .get_document_for_session(session, &statement.table, &row_id)
            .map_err(|error| QueryError::General(error.to_string()))?
            .ok_or_else(|| {
                QueryError::General(format!(
                    "row '{row_id}' was not found in '{}'",
                    statement.table
                ))
            })?;
        let mut payload =
            current.payload.as_object().cloned().ok_or_else(|| {
                QueryError::General("stored row payload must be object".to_string())
            })?;

        for (field, expr) in &statement.assignments {
            let value = filter::evaluate_expr_value(
                row,
                expr,
                params,
                None,
                user_functions,
                session,
                None,
            )?;
            payload.insert(
                field.clone(),
                update_assignment_to_json(field, &value, &schema),
            );
        }

        let payload = cassie
            .prepare_document_write_for_session(
                session,
                &statement.table,
                serde_json::Value::Object(payload),
                true,
                Some(&row_id),
            )
            .map_err(|error| QueryError::General(error.to_string()))?;
        prepared_rows.push((row_id, payload));
    }

    let mut returning_rows = Vec::new();
    for (row_id, payload) in prepared_rows {
        cassie
            .put_prepared_document_for_session(session, &statement.table, row_id.clone(), payload)
            .map_err(|error| QueryError::General(error.to_string()))?;

        if !statement.returning.is_empty() {
            let document = cassie
                .get_document_for_session(session, &statement.table, &row_id)
                .map_err(|error| QueryError::General(error.to_string()))?
                .ok_or_else(|| {
                    QueryError::General(format!(
                        "updated row '{row_id}' was not found in '{}'",
                        statement.table
                    ))
                })?;
            returning_rows.push(inserted_row_to_batch_row(
                &row_id,
                &schema,
                &document.payload,
            ));
        }
    }

    let updated_count = if statement.returning.is_empty() {
        matched_rows.len()
    } else {
        returning_rows.len()
    };
    if statement.returning.is_empty() {
        return Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: format!("UPDATE {updated_count}"),
        });
    }

    let projected = projection::project_rows(
        returning_rows,
        &statement.returning,
        params,
        None,
        user_functions,
        session,
    )?;

    let column_schema = cassie.catalog.get_schema(&statement.table);
    let columns =
        dml_returning_columns(&statement.returning, column_schema.as_ref(), user_functions);

    Ok(QueryResult {
        columns,
        rows: projected.into_iter().map(BatchRow::into_values).collect(),
        command: format!("UPDATE {updated_count}"),
    })
}

fn row_id_from_batch_row(row: &BatchRow) -> Result<String, QueryError> {
    match row.get("id") {
        Some(Value::String(value)) if !value.is_empty() => Ok(value.clone()),
        _ => Err(QueryError::General(
            "scanned row is missing internal row id".to_string(),
        )),
    }
}

fn execute_delete(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::DeleteStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    check_timeout(controls)?;
    let schema = cassie.catalog.get_schema(&statement.table).ok_or_else(|| {
        QueryError::General(format!("collection '{}' not found", statement.table))
    })?;

    let batches = scan::scan(cassie, session, &statement.table)?;
    ensure_temp_budget(controls, &batches)?;
    let rows = batch::flatten_batches(batches);
    let matched_rows = if let Some(filter_expr) = &statement.filter {
        filter::filter_rows(rows, filter_expr, params, None, user_functions, session)?
    } else {
        rows
    };

    let mut delete_ids = Vec::with_capacity(matched_rows.len());
    let mut returning_rows = Vec::new();
    for row in &matched_rows {
        let row_id = row_id_from_batch_row(row)?;
        if !statement.returning.is_empty() {
            let current = cassie
                .get_document_for_session(session, &statement.table, &row_id)
                .map_err(|error| QueryError::General(error.to_string()))?
                .ok_or_else(|| {
                    QueryError::General(format!(
                        "row '{row_id}' was not found in '{}'",
                        statement.table
                    ))
                })?;
            returning_rows.push(inserted_row_to_batch_row(
                &row_id,
                &schema,
                &current.payload,
            ));
        }
        delete_ids.push(row_id);
    }

    for row_id in &delete_ids {
        cassie
            .delete_document_for_session(session, &statement.table, row_id)
            .map_err(|error| QueryError::General(error.to_string()))?;
    }

    let deleted_count = delete_ids.len();
    if statement.returning.is_empty() {
        return Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: format!("DELETE {deleted_count}"),
        });
    }

    let projected = projection::project_rows(
        returning_rows,
        &statement.returning,
        params,
        None,
        user_functions,
        session,
    )?;

    let column_schema = cassie.catalog.get_schema(&statement.table);
    let columns =
        dml_returning_columns(&statement.returning, column_schema.as_ref(), user_functions);

    Ok(QueryResult {
        columns,
        rows: projected.into_iter().map(BatchRow::into_values).collect(),
        command: format!("DELETE {deleted_count}"),
    })
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
