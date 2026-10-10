use super::{
    build_dml_result, check_timeout, dml_referential_actions, execute_plan, filter,
    inserted_row_to_batch_row, integral_json_number, json_to_value, update_assignment_to_json,
    value_to_json_for_field, BatchRow, Cassie, CassieSession, CollectionSchema, CteContext,
    DataType, DmlResultContext, Expr, FieldMeta, FunctionMeta, HashMap, InsertSource, LogicalPlan,
    QueryError, QueryExecutionControls, QueryResult, QuerySource, Value,
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
    let returning_read_controls =
        if crate::executor::execution::exists_projection::contains(&statement.returning) {
            Some(
                crate::executor::execution::entrypoints::statement_read_controls(
                    cassie, session, controls,
                )?,
            )
        } else {
            None
        };
    let statement_read_controls = returning_read_controls.as_ref().unwrap_or(controls);
    let schema = cassie.catalog.get_schema(&statement.table).ok_or_else(|| {
        QueryError::General(format!("collection '{}' not found", statement.table))
    })?;
    let source_rows = insert_source_rows(
        cassie,
        session,
        statement,
        params,
        user_functions,
        statement_read_controls,
    )?;
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
        controls,
    };
    let mut affected_row_ids = std::collections::HashSet::new();
    for source_row in source_rows {
        check_timeout(controls)?;
        let Some(row_id) = execute_insert_source_row(
            &insert_context,
            &target_fields,
            &source_row,
            &affected_row_ids,
        )?
        else {
            continue;
        };
        affected_row_ids.insert(row_id.clone());
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
            controls,
            statement_read_controls,
        },
        affected_count,
        returning_rows,
    )
}

fn find_target_field_conflict_row_id(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    target_fields: &[String],
    object: &serde_json::Map<String, serde_json::Value>,
    schema: &CollectionSchema,
) -> Result<Option<String>, QueryError> {
    let mut values = Vec::with_capacity(target_fields.len());
    for field in target_fields {
        let Some(value) = object.get(field) else {
            return Ok(None);
        };
        if crate::app::field_value_is_sql_null(Some(value), Some(schema), field) {
            return Ok(None);
        }
        values.push((field.as_str(), value));
    }
    cassie
        .find_document_id_by_fields(session, table, &values, None)
        .map_err(QueryError::from)
}

fn find_insert_conflict_row_id(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::InsertStatement,
    payload: &serde_json::Value,
    schema: &CollectionSchema,
) -> Result<Option<String>, QueryError> {
    let Some(on_conflict) = statement.on_conflict.as_ref() else {
        return Ok(None);
    };
    let row_schema = cassie
        .midge
        .row_schema(&statement.table)
        .map_err(QueryError::from)?;
    let mut canonical_payload = payload.clone();
    if let Some(object) = canonical_payload.as_object_mut() {
        for (field, value) in object {
            crate::executor::execution::index_probe_canonicalization::canonicalize_index_probe_value(
                cassie,
                &statement.table,
                &crate::sql::ColumnIdentifierPath::stored_field_key(field),
                value,
            );
        }
    }
    let object = canonical_payload
        .as_object()
        .ok_or_else(|| QueryError::General("document payload must be an object".to_string()))?;

    if !on_conflict.target_fields.is_empty() {
        return find_target_field_conflict_row_id(
            cassie,
            session,
            &statement.table,
            &on_conflict.target_fields,
            object,
            schema,
        );
    }

    let constraints = cassie.catalog.get_constraints(&statement.table);
    for constraint in &constraints {
        if !crate::catalog::enforces_single_column_uniqueness(constraint, &constraints) {
            continue;
        }
        let Some(value) = object.get(&constraint.field) else {
            continue;
        };
        if crate::app::field_value_is_sql_null(Some(value), Some(schema), &constraint.field) {
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
            if let Some(id) = find_expression_index_conflict(
                cassie,
                session,
                &statement.table,
                &index,
                payload,
                &row_schema,
            )? {
                return Ok(Some(id));
            }
            continue;
        }
        let mut values = Vec::with_capacity(fields.len());
        let mut complete = true;
        for field in &fields {
            let Some(value) = object.get(field) else {
                complete = false;
                break;
            };
            if crate::app::field_value_is_sql_null(Some(value), Some(schema), field) {
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

fn excluded_local_args(
    payload: &serde_json::Value,
    schema: &CollectionSchema,
) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    let Some(object) = payload.as_object() else {
        return out;
    };
    for (field, value) in object {
        let value = schema
            .fields
            .iter()
            .find(|candidate| candidate.name == *field)
            .filter(|candidate| {
                matches!(candidate.data_type, DataType::Json)
                    && crate::types::json::requires_document_carrier(value)
            })
            .map_or_else(|| json_to_value(value), |_| Value::Json(value.clone()));
        let key = crate::sql::ColumnIdentifierPath::from_field_name(field).lookup_key();
        out.insert(format!("excluded.{key}"), value);
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
                    .map(|expr| insert_expr_to_value(expr, params).map_err(QueryError::General))
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect::<Result<Vec<_>, _>>(),
        InsertSource::Select(select) => {
            let statement_controls =
                crate::executor::execution::entrypoints::statement_read_controls(
                    cassie, session, controls,
                )?;
            let controls = &statement_controls;
            let (_read_scope, _overlay_scope) =
                crate::executor::execution::entrypoints::enter_statement_read(controls);
            let logical = LogicalPlan {
                command: None,
                source: select.source.clone(),
                collection: match &select.source {
                    QuerySource::Aliased { source, .. } => {
                        crate::planner::logical::source_name(source)
                    }
                    QuerySource::Collection(name) => name.to_string(),
                    QuerySource::Cte(name) | QuerySource::TableFunction { name, .. } => {
                        name.clone()
                    }
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
                limit: select.limit.clone(),
                offset: select.offset.clone(),
                set: select.set.clone(),
            };
            // Constructed here, so it needs the reserved-id rewrite too —
            // otherwise `INSERT ... SELECT id` stores NULL instead of the
            // source rows' internal identity.
            let mut logical = logical;
            crate::planner::logical::rewrite_reserved_id_references(&mut logical, &cassie.catalog);
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
                    let _operator_parent = row.operator_memory();
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
) -> Result<serde_json::Map<String, serde_json::Value>, QueryError> {
    let mut payload = serde_json::Map::with_capacity(target_fields.len());
    for (field, value) in target_fields.iter().zip(source_row.iter()) {
        if matches!(field.data_type, DataType::Json) && matches!(value, Value::Null) {
            continue;
        }
        payload.insert(
            field.name.clone(),
            value_to_json_for_field(&field.name, value, &field.data_type)?,
        );
    }
    Ok(payload)
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
    controls: &'a QueryExecutionControls,
}

fn execute_insert_source_row(
    context: &InsertExecutionContext<'_>,
    target_fields: &[FieldMeta],
    source_row: &[Value],
    affected_row_ids: &std::collections::HashSet<String>,
) -> Result<Option<String>, QueryError> {
    let payload = serde_json::Value::Object(payload_from_insert_row(target_fields, source_row)?);
    let maybe_conflict_id = find_insert_conflict_row_id(
        context.cassie,
        context.session,
        context.statement,
        &payload,
        context.schema,
    )?;
    match (context.statement.on_conflict.as_ref(), maybe_conflict_id) {
        (Some(on_conflict), Some(conflict_id)) => match &on_conflict.action {
            crate::sql::ast::InsertConflictAction::DoNothing => Ok(None),
            crate::sql::ast::InsertConflictAction::DoUpdate {
                assignments,
                filter,
            } => {
                reject_second_conflict_update(&conflict_id, affected_row_ids)?;
                execute_insert_conflict_update(
                    context,
                    &payload,
                    &conflict_id,
                    assignments,
                    filter.as_ref(),
                )
            }
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
                resolve_autocommit_insert_conflict(context, &payload, error, affected_row_ids)
            }
            Err(error) => Err(QueryError::from(error)),
        },
    }
}

pub(crate) fn resolve_transaction_conflict_intents(
    cassie: &Cassie,
    session: &CassieSession,
    controls: Option<&QueryExecutionControls>,
) -> Result<(), QueryError> {
    let fallback_controls = controls
        .is_none()
        .then(|| cassie.runtime.query_controls(std::time::Instant::now()));
    let controls = controls
        .or(fallback_controls.as_ref())
        .expect("fallback controls are present when caller controls are absent");
    for intent in session.transaction_conflict_intents() {
        session.remove_document_change(&intent.statement.table, &intent.provisional_id);
        let context = InsertExecutionContext {
            cassie,
            session: Some(session),
            statement: &intent.statement,
            params: &intent.params,
            user_functions: &intent.user_functions,
            schema: &intent.schema,
            controls,
        };
        let Some(conflict_id) = find_insert_conflict_row_id(
            cassie,
            Some(session),
            &intent.statement,
            &intent.payload,
            &intent.schema,
        )?
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

/// Finds the row an expression unique index says `payload` conflicts with,
/// by reading its committed reservation owner and reconciling staged rows.
fn find_expression_index_conflict(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    index: &crate::catalog::IndexMeta,
    payload: &serde_json::Value,
    row_schema: &crate::midge::row_blob::RowSchema,
) -> Result<Option<String>, QueryError> {
    use crate::midge::adapter::Midge;
    if !Midge::payload_matches_scalar_index_predicate(index, payload, row_schema)? {
        return Ok(None);
    }
    let Some(key) = Midge::scalar_index_key_values(index, payload, row_schema)? else {
        return Ok(None);
    };
    let staged_snapshot = session.map(|session| session.staged_write_snapshot(table));
    let staged_changes = staged_snapshot
        .as_ref()
        .map(crate::app::StagedWriteSnapshot::ordered_changes);
    let committed_owner =
        cassie
            .midge
            .unique_scalar_index_reservation_owner(table, &index.name, &key)?;

    if let Some(owner_id) = committed_owner.as_ref() {
        match staged_changes.and_then(|changes| changes.get(owner_id)) {
            None => return Ok(Some(owner_id.clone())),
            Some(crate::app::TransactionRowChange::Upsert(owner_payload))
                if expression_index_key_matches(index, owner_payload, row_schema, &key)? =>
            {
                return Ok(Some(owner_id.clone()));
            }
            Some(_) => {}
        }
    }

    if let Some(changes) = staged_changes {
        for (id, change) in changes {
            let crate::app::TransactionRowChange::Upsert(staged_payload) = change else {
                continue;
            };
            if expression_index_key_matches(index, staged_payload, row_schema, &key)? {
                return Ok(Some(id.clone()));
            }
        }
    }
    Ok(None)
}

fn expression_index_key_matches(
    index: &crate::catalog::IndexMeta,
    payload: &serde_json::Value,
    row_schema: &crate::midge::row_blob::RowSchema,
    key: &[serde_json::Value],
) -> Result<bool, QueryError> {
    use crate::midge::adapter::Midge;
    if !Midge::payload_matches_scalar_index_predicate(index, payload, row_schema)? {
        return Ok(false);
    }
    Ok(Midge::scalar_index_key_values(index, payload, row_schema)?.as_deref() == Some(key))
}

/// Like PostgreSQL, one INSERT ... ON CONFLICT DO UPDATE may not affect the
/// same row twice, whether it inserted that row earlier in the statement or
/// already updated it.
fn reject_second_conflict_update(
    conflict_id: &str,
    affected_row_ids: &std::collections::HashSet<String>,
) -> Result<(), QueryError> {
    if affected_row_ids.contains(conflict_id) {
        return Err(QueryError::Cassie(
            crate::app::CassieError::CardinalityViolation(
                "ON CONFLICT DO UPDATE command cannot affect row a second time".to_string(),
            ),
        ));
    }
    Ok(())
}

fn resolve_autocommit_insert_conflict(
    context: &InsertExecutionContext<'_>,
    payload: &serde_json::Value,
    original_error: crate::app::CassieError,
    affected_row_ids: &std::collections::HashSet<String>,
) -> Result<Option<String>, QueryError> {
    let on_conflict = context
        .statement
        .on_conflict
        .as_ref()
        .expect("conflict clause checked by caller");
    let Some(conflict_id) = find_insert_conflict_row_id(
        context.cassie,
        context.session,
        context.statement,
        payload,
        context.schema,
    )?
    else {
        // Untargeted DO NOTHING skips a conflict on any unique key, including
        // expression indexes whose conflicting row cannot be looked up here.
        if on_conflict.target_fields.is_empty()
            && matches!(
                on_conflict.action,
                crate::sql::ast::InsertConflictAction::DoNothing
            )
        {
            return Ok(None);
        }
        // The violation is on a key other than the ON CONFLICT target, so it
        // stands, and keeps its 23505 SQLSTATE.
        return Err(QueryError::from(original_error));
    };
    match &on_conflict.action {
        crate::sql::ast::InsertConflictAction::DoNothing => Ok(None),
        crate::sql::ast::InsertConflictAction::DoUpdate {
            assignments,
            filter,
        } => {
            reject_second_conflict_update(&conflict_id, affected_row_ids)?;
            execute_insert_conflict_update(
                context,
                payload,
                &conflict_id,
                assignments,
                filter.as_ref(),
            )
        }
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
    let _type_memory = context
        .controls
        .reserve_query_memory(crate::executor::batch::row_type_bytes([&existing_row]))?;
    let excluded_args = excluded_local_args(payload, context.schema);
    if let Some(filter_expr) = conflict_filter {
        let filter_expr = crate::executor::execution::resolve_statement_exists(
            context.cassie,
            context.session,
            filter_expr,
            context.user_functions,
            context.params,
            context.controls,
        )?;
        let matches = filter::eval_scalar(
            &existing_row,
            &filter_expr,
            context.params,
            None,
            context.user_functions,
            Some(&excluded_args),
            context.session,
        )?
        .is_true()?;
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
    let mut merged_payload = serde_json::Value::Object(merged_payload);
    context.cassie.discard_stale_vector_embeddings(
        &context.statement.table,
        &mut merged_payload,
        assignments.iter().map(|(field, _)| field.as_str()),
    );
    let prepared = context
        .cassie
        .prepare_document_write_for_session(
            context.session,
            &context.statement.table,
            merged_payload,
            false,
            Some(conflict_id),
        )
        .map_err(QueryError::from)?;
    dml_referential_actions::update_existing_row(
        context.cassie,
        context.session,
        &context.statement.table,
        conflict_id,
        &current.payload,
        prepared,
        context.controls,
    )?;
    Ok(Some(conflict_id.to_string()))
}

fn conflict_existing_row(
    row_id: &str,
    schema: &CollectionSchema,
    payload: &serde_json::Value,
    table: &str,
) -> BatchRow {
    let mut row = inserted_row_to_batch_row(row_id, schema, payload);
    super::scan::attach_row_types(&mut row, Some(schema));
    let types = row.shared_data_types();
    let (values, mut aliases) = row.into_parts();
    let local_table = crate::catalog::local_name(table);
    for (index, (field, _)) in values.iter().enumerate() {
        aliases.push((format!("{table}.{field}"), index));
        aliases.push((format!("{local_table}.{field}"), index));
    }
    BatchRow::with_aliases(values, aliases).with_optional_data_types(types)
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
            update_assignment_to_json(field, &value, context.schema)?,
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
            .get(
                &crate::sql::ColumnIdentifierPath::parse(name)
                    .map_or_else(|_| name.to_ascii_lowercase(), |column| column.lookup_key()),
            )
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
                .find(|field| field.name == *column)
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

fn insert_expr_to_value(expr: &Expr, params: &[Value]) -> Result<Value, String> {
    match expr {
        Expr::StringLiteral(value) => Ok(Value::String(value.clone())),
        Expr::NumberLiteral(value) => {
            number_literal_to_json(*value).map(|value| json_to_value(&value))
        }
        Expr::IntegerLiteral(value) => Ok(Value::Int64(*value)),
        Expr::BoolLiteral(value) => Ok(Value::Bool(*value)),
        Expr::Null => Ok(Value::Null),
        Expr::Param(index) => params
            .get(*index)
            .ok_or_else(|| format!("missing bind parameter ${}", index + 1))
            .cloned(),
        Expr::Column(_)
        | Expr::Case { .. }
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
