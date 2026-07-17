use super::{
    build_dml_result, check_timeout, execute_plan, filter, inserted_row_to_batch_row,
    integral_json_number, json_to_value, update_assignment_to_json, value_to_json, BatchRow,
    Cassie, CassieSession, CollectionSchema, CteContext, DmlResultContext, Expr, FieldMeta,
    FunctionMeta, HashMap, InsertSource, LogicalPlan, QueryError, QueryExecutionControls,
    QueryResult, QuerySource, Value,
};

pub(in crate::executor::execution) fn execute_insert(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::InsertStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    check_timeout(controls)?;
    let schema = cassie.catalog.get_schema(&statement.table).ok_or_else(|| {
        QueryError::General(format!("collection '{}' not found", statement.table))
    })?;
    let source_rows =
        insert_source_rows(cassie, session, statement, params, user_functions, controls)?;
    let source_width = source_rows
        .first()
        .map_or_else(|| insert_source_width(statement, &schema), Vec::len);
    let target_fields = insert_target_fields(statement, &schema, source_width)?;
    validate_insert_source_rows(&source_rows, target_fields.len())?;

    let mut affected_count = 0usize;
    let mut returning_rows = Vec::new();
    let insert_context = InsertExecutionContext {
        cassie,
        session,
        statement,
        params,
        user_functions,
        schema: &schema,
    };
    for source_row in source_rows {
        check_timeout(controls)?;
        let Some(row_id) = execute_insert_source_row(&insert_context, &target_fields, &source_row)?
        else {
            continue;
        };
        affected_count += 1;
        append_insert_returning_row(
            cassie,
            session,
            statement,
            &schema,
            &row_id,
            &mut returning_rows,
        )?;
    }
    build_dml_result(
        &DmlResultContext {
            cassie,
            session,
            table: &statement.table,
            returning: &statement.returning,
            params,
            user_functions,
            command_prefix: "INSERT 0",
        },
        affected_count,
        returning_rows,
    )
}

fn find_insert_conflict_row_id(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::InsertStatement,
    payload: &serde_json::Value,
) -> Result<Option<String>, QueryError> {
    let Some(on_conflict) = statement.on_conflict.as_ref() else {
        return Ok(None);
    };
    let object = payload
        .as_object()
        .ok_or_else(|| QueryError::General("document payload must be an object".to_string()))?;

    if !on_conflict.target_fields.is_empty() {
        let values = on_conflict
            .target_fields
            .iter()
            .map(|field| {
                object
                    .get(field)
                    .map(|value| (field.as_str(), value))
                    .ok_or_else(|| {
                        QueryError::General(format!(
                            "ON CONFLICT target column '{field}' is missing from inserted row"
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        return cassie
            .find_document_id_by_fields(session, &statement.table, &values, None)
            .map_err(QueryError::from);
    }

    for constraint in cassie.catalog.get_constraints(&statement.table) {
        if !(constraint.primary_key || constraint.unique) {
            continue;
        }
        let Some(value) = object.get(&constraint.field) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        if let Some(id) = cassie
            .find_document_id_by_fields(
                session,
                &statement.table,
                &[(&constraint.field, value)],
                None,
            )
            .map_err(QueryError::from)?
        {
            return Ok(Some(id));
        }
    }

    for index in cassie.catalog.list_indexes(&statement.table) {
        if !index.unique || index.kind != crate::catalog::IndexKind::Scalar {
            continue;
        }
        let fields = index.normalized_fields();
        if fields.is_empty() {
            continue;
        }
        let mut values = Vec::with_capacity(fields.len());
        let mut complete = true;
        for field in &fields {
            let Some(value) = object.get(field) else {
                complete = false;
                break;
            };
            if value.is_null() {
                complete = false;
                break;
            }
            values.push((field.as_str(), value));
        }
        if !complete {
            continue;
        }
        if let Some(id) = cassie
            .find_document_id_by_fields(session, &statement.table, &values, None)
            .map_err(QueryError::from)?
        {
            return Ok(Some(id));
        }
    }

    Ok(None)
}

fn excluded_local_args(payload: &serde_json::Value) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    let Some(object) = payload.as_object() else {
        return out;
    };
    for (field, value) in object {
        out.insert(
            format!("excluded.{}", field.to_ascii_lowercase()),
            json_to_value(value),
        );
    }
    out
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
                    QuerySource::Collection(name)
                    | QuerySource::Cte(name)
                    | QuerySource::TableFunction { name, .. } => name.clone(),
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

fn validate_insert_source_rows(
    source_rows: &[Vec<Value>],
    target_field_count: usize,
) -> Result<(), QueryError> {
    for row in source_rows {
        if row.len() != target_field_count {
            return Err(QueryError::General(format!(
                "INSERT column/value counts mismatch: {} columns, {} values",
                target_field_count,
                row.len()
            )));
        }
    }
    Ok(())
}

struct InsertExecutionContext<'a> {
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    statement: &'a crate::sql::ast::InsertStatement,
    params: &'a [Value],
    user_functions: &'a HashMap<String, FunctionMeta>,
    schema: &'a CollectionSchema,
}

fn execute_insert_source_row(
    context: &InsertExecutionContext<'_>,
    target_fields: &[FieldMeta],
    source_row: &[Value],
) -> Result<Option<String>, QueryError> {
    let payload = serde_json::Value::Object(payload_from_insert_row(target_fields, source_row));
    let maybe_conflict_id =
        find_insert_conflict_row_id(context.cassie, context.session, context.statement, &payload)?;
    match (context.statement.on_conflict.as_ref(), maybe_conflict_id) {
        (Some(on_conflict), Some(conflict_id)) => match &on_conflict.action {
            crate::sql::ast::InsertConflictAction::DoNothing => Ok(None),
            crate::sql::ast::InsertConflictAction::DoUpdate {
                assignments,
                filter,
            } => execute_insert_conflict_update(
                context,
                &payload,
                &conflict_id,
                assignments,
                filter.as_ref(),
            ),
        },
        (_, Some(_)) => Err(QueryError::General(
            "INSERT conflict detected without ON CONFLICT clause".to_string(),
        )),
        (_, None) => match context.cassie.write_document_for_session(
            context.session,
            &context.statement.table,
            None,
            payload.clone(),
            true,
            None,
        ) {
            Ok(row_id) => {
                if context.statement.on_conflict.is_some() {
                    if let Some(session) = context
                        .session
                        .filter(|session| session.is_transaction_active())
                    {
                        session.stage_conflict_intent(crate::app::TransactionConflictIntent {
                            provisional_id: row_id.clone(),
                            statement: context.statement.clone(),
                            payload,
                            params: context.params.to_vec(),
                            user_functions: context.user_functions.clone(),
                            schema: context.schema.clone(),
                        });
                    }
                }
                Ok(Some(row_id))
            }
            Err(error @ crate::app::CassieError::UniqueViolation { .. })
                if context.statement.on_conflict.is_some()
                    && !context
                        .session
                        .is_some_and(CassieSession::is_transaction_active) =>
            {
                resolve_autocommit_insert_conflict(context, &payload, &error)
            }
            Err(error) => Err(QueryError::from(error)),
        },
    }
}

pub(crate) fn resolve_transaction_conflict_intents(
    cassie: &Cassie,
    session: &CassieSession,
) -> Result<(), QueryError> {
    for intent in session.transaction_conflict_intents() {
        session.remove_document_change(&intent.statement.table, &intent.provisional_id);
        let context = InsertExecutionContext {
            cassie,
            session: Some(session),
            statement: &intent.statement,
            params: &intent.params,
            user_functions: &intent.user_functions,
            schema: &intent.schema,
        };
        let Some(conflict_id) =
            find_insert_conflict_row_id(cassie, Some(session), &intent.statement, &intent.payload)?
        else {
            session
                .stage_document_write(
                    &intent.statement.table,
                    intent.provisional_id,
                    intent.payload,
                )
                .map_err(QueryError::from)?;
            continue;
        };
        let on_conflict = intent
            .statement
            .on_conflict
            .as_ref()
            .expect("transaction conflict intent has a conflict clause");
        if let crate::sql::ast::InsertConflictAction::DoUpdate {
            assignments,
            filter,
        } = &on_conflict.action
        {
            execute_insert_conflict_update(
                &context,
                &intent.payload,
                &conflict_id,
                assignments,
                filter.as_ref(),
            )?;
        }
    }
    session.clear_conflict_intents();
    Ok(())
}

fn resolve_autocommit_insert_conflict(
    context: &InsertExecutionContext<'_>,
    payload: &serde_json::Value,
    original_error: &crate::app::CassieError,
) -> Result<Option<String>, QueryError> {
    let Some(conflict_id) =
        find_insert_conflict_row_id(context.cassie, context.session, context.statement, payload)?
    else {
        return Err(QueryError::General(original_error.to_string()));
    };
    let on_conflict = context
        .statement
        .on_conflict
        .as_ref()
        .expect("conflict clause checked by caller");
    match &on_conflict.action {
        crate::sql::ast::InsertConflictAction::DoNothing => Ok(None),
        crate::sql::ast::InsertConflictAction::DoUpdate {
            assignments,
            filter,
        } => execute_insert_conflict_update(
            context,
            payload,
            &conflict_id,
            assignments,
            filter.as_ref(),
        ),
    }
}

struct ConflictAssignmentContext<'a> {
    existing_row: &'a BatchRow,
    excluded_args: &'a HashMap<String, Value>,
    params: &'a [Value],
    user_functions: &'a HashMap<String, FunctionMeta>,
    session: Option<&'a CassieSession>,
    schema: &'a CollectionSchema,
}

fn execute_insert_conflict_update(
    context: &InsertExecutionContext<'_>,
    payload: &serde_json::Value,
    conflict_id: &str,
    assignments: &[(String, Expr)],
    conflict_filter: Option<&Expr>,
) -> Result<Option<String>, QueryError> {
    let current = context
        .cassie
        .get_document_for_session(context.session, &context.statement.table, conflict_id)
        .map_err(QueryError::from)?
        .ok_or_else(|| {
            QueryError::General(format!(
                "conflicting row '{conflict_id}' was not found in '{}'",
                context.statement.table
            ))
        })?;
    let existing_row = conflict_existing_row(
        conflict_id,
        context.schema,
        &current.payload,
        &context.statement.table,
    );
    let excluded_args = excluded_local_args(payload);
    if let Some(filter_expr) = conflict_filter {
        let matches = filter::eval_scalar(
            &existing_row,
            filter_expr,
            context.params,
            None,
            context.user_functions,
            Some(&excluded_args),
            context.session,
        )?
        .is_true();
        if !matches {
            return Ok(None);
        }
    }
    let assignment_context = ConflictAssignmentContext {
        existing_row: &existing_row,
        excluded_args: &excluded_args,
        params: context.params,
        user_functions: context.user_functions,
        session: context.session,
        schema: context.schema,
    };
    let merged_payload =
        merged_conflict_payload(&current.payload, assignments, &assignment_context)?;
    let prepared = context
        .cassie
        .prepare_document_write_for_session(
            context.session,
            &context.statement.table,
            serde_json::Value::Object(merged_payload),
            true,
            Some(conflict_id),
        )
        .map_err(QueryError::from)?;
    context
        .cassie
        .put_prepared_document_for_session(
            context.session,
            &context.statement.table,
            conflict_id.to_string(),
            prepared,
        )
        .map_err(QueryError::from)?;
    Ok(Some(conflict_id.to_string()))
}

fn conflict_existing_row(
    row_id: &str,
    schema: &CollectionSchema,
    payload: &serde_json::Value,
    table: &str,
) -> BatchRow {
    let row = inserted_row_to_batch_row(row_id, schema, payload);
    let (values, mut aliases) = row.into_parts();
    let local_table = table.rsplit('.').next().unwrap_or(table);
    for (index, (field, _)) in values.iter().enumerate() {
        aliases.push((format!("{table}.{field}"), index));
        aliases.push((format!("{local_table}.{field}"), index));
    }
    BatchRow::with_aliases(values, aliases)
}

fn merged_conflict_payload(
    current_payload: &serde_json::Value,
    assignments: &[(String, Expr)],
    context: &ConflictAssignmentContext<'_>,
) -> Result<serde_json::Map<String, serde_json::Value>, QueryError> {
    let mut merged_payload = current_payload
        .as_object()
        .cloned()
        .ok_or_else(|| QueryError::General("stored row payload must be object".to_string()))?;
    for (field, expr) in assignments {
        let value = conflict_assignment_value(
            expr,
            context.existing_row,
            context.excluded_args,
            context.params,
            context.user_functions,
            context.session,
        )?;
        merged_payload.insert(
            field.clone(),
            update_assignment_to_json(field, &value, context.schema),
        );
    }
    Ok(merged_payload)
}

fn conflict_assignment_value(
    expr: &Expr,
    existing_row: &BatchRow,
    excluded_args: &HashMap<String, Value>,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Value, QueryError> {
    match expr {
        Expr::Column(name) => Ok(excluded_args
            .get(&name.to_ascii_lowercase())
            .cloned()
            .or_else(|| existing_row.get(name).cloned())
            .unwrap_or(Value::Null)),
        _ => filter::evaluate_expr_value(
            existing_row,
            expr,
            params,
            None,
            user_functions,
            session,
            Some(excluded_args),
        ),
    }
}

fn append_insert_returning_row(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::InsertStatement,
    schema: &CollectionSchema,
    row_id: &str,
    returning_rows: &mut Vec<BatchRow>,
) -> Result<(), QueryError> {
    if statement.returning.is_empty() || row_id.is_empty() {
        return Ok(());
    }
    let document = cassie
        .get_document_for_session(session, &statement.table, row_id)
        .map_err(QueryError::from)?
        .ok_or_else(|| {
            QueryError::General(format!(
                "affected row '{row_id}' was not found in '{}'",
                statement.table
            ))
        })?;
    returning_rows.push(inserted_row_to_batch_row(row_id, schema, &document.payload));
    Ok(())
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
    if let Some(integer) = integral_json_number(value) {
        return Ok(serde_json::Value::Number(integer));
    }
    serde_json::Number::from_f64(value)
        .map(serde_json::Value::Number)
        .ok_or_else(|| "INSERT VALUES requires finite numeric literals".to_string())
}
