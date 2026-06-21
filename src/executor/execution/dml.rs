use super::*;

pub(super) fn execute_insert(
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

    let mut affected_count = 0usize;
    let mut returning_rows = Vec::new();
    for source_row in source_rows {
        let payload = payload_from_insert_row(&target_fields, &source_row);
        let payload = serde_json::Value::Object(payload);
        let maybe_conflict_id = find_insert_conflict_row_id(cassie, session, statement, &payload)?;

        let row_id = match (statement.on_conflict.as_ref(), maybe_conflict_id) {
            (Some(on_conflict), Some(conflict_id)) => match &on_conflict.action {
                crate::sql::ast::InsertConflictAction::DoNothing => {
                    continue;
                }
                crate::sql::ast::InsertConflictAction::DoUpdate {
                    assignments,
                    filter,
                } => {
                    let current = cassie
                        .get_document_for_session(session, &statement.table, &conflict_id)
                        .map_err(|error| QueryError::General(error.to_string()))?
                        .ok_or_else(|| {
                            QueryError::General(format!(
                                "conflicting row '{conflict_id}' was not found in '{}'",
                                statement.table
                            ))
                        })?;
                    let existing_row =
                        inserted_row_to_batch_row(&conflict_id, &schema, &current.payload);
                    let excluded_args = excluded_local_args(&payload);

                    if let Some(filter) = filter {
                        let matches = filter::eval_scalar(
                            &existing_row,
                            filter,
                            params,
                            None,
                            user_functions,
                            Some(&excluded_args),
                            session,
                        )?
                        .as_bool();
                        if !matches {
                            continue;
                        }
                    }

                    let mut merged_payload =
                        current.payload.as_object().cloned().ok_or_else(|| {
                            QueryError::General("stored row payload must be object".to_string())
                        })?;
                    for (field, expr) in assignments {
                        let value = match expr {
                            Expr::Column(name) => excluded_args
                                .get(&name.to_ascii_lowercase())
                                .cloned()
                                .or_else(|| existing_row.get(name).cloned())
                                .unwrap_or(Value::Null),
                            _ => filter::evaluate_expr_value(
                                &existing_row,
                                expr,
                                params,
                                None,
                                user_functions,
                                session,
                                Some(&excluded_args),
                            )?,
                        };
                        merged_payload.insert(
                            field.clone(),
                            update_assignment_to_json(field, &value, &schema),
                        );
                    }

                    let prepared = cassie
                        .prepare_document_write_for_session(
                            session,
                            &statement.table,
                            serde_json::Value::Object(merged_payload),
                            true,
                            Some(&conflict_id),
                        )
                        .map_err(|error| QueryError::General(error.to_string()))?;
                    cassie
                        .put_prepared_document_for_session(
                            session,
                            &statement.table,
                            conflict_id.clone(),
                            prepared,
                        )
                        .map_err(|error| QueryError::General(error.to_string()))?;
                    conflict_id
                }
            },
            (_, Some(_)) => {
                return Err(QueryError::General(
                    "INSERT conflict detected without ON CONFLICT clause".to_string(),
                ));
            }
            (_, None) => cassie
                .write_document_for_session(session, &statement.table, None, payload, true, None)
                .map_err(|error| QueryError::General(error.to_string()))?,
        };
        affected_count += 1;

        if !statement.returning.is_empty() {
            let document = cassie
                .get_document_for_session(session, &statement.table, &row_id)
                .map_err(|error| QueryError::General(error.to_string()))?
                .ok_or_else(|| {
                    QueryError::General(format!(
                        "affected row '{row_id}' was not found in '{}'",
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
            command: format!("INSERT 0 {affected_count}"),
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
        command: format!("INSERT 0 {affected_count}"),
    })
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
                            "ON CONFLICT target column '{}' is missing from inserted row",
                            field
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        return cassie
            .find_document_id_by_fields(session, &statement.table, &values, None)
            .map_err(|error| QueryError::General(error.to_string()));
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
            .map_err(|error| QueryError::General(error.to_string()))?
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
            .map_err(|error| QueryError::General(error.to_string()))?
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

fn referencing_constraints(
    cassie: &Cassie,
    referenced_table: &str,
) -> Vec<(String, crate::catalog::FieldConstraint)> {
    let mut out = Vec::new();
    for collection in cassie.catalog.list_collections() {
        for constraint in cassie.catalog.get_constraints(&collection.name) {
            if constraint
                .references_table
                .as_deref()
                .is_some_and(|table| table.eq_ignore_ascii_case(referenced_table))
            {
                out.push((collection.name.clone(), constraint));
            }
        }
    }
    out
}

fn assert_no_referencing_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    payload: &serde_json::Value,
) -> Result<(), QueryError> {
    let Some(object) = payload.as_object() else {
        return Ok(());
    };

    for (child_table, constraint) in referencing_constraints(cassie, table) {
        let Some(reference_field) = constraint.references_field.as_deref() else {
            continue;
        };
        let Some(parent_value) = object.get(reference_field) else {
            continue;
        };
        if parent_value.is_null() {
            continue;
        }
        if cassie
            .value_exists_for_collection_field(
                session,
                &child_table,
                &constraint.field,
                parent_value,
                None,
            )
            .map_err(|error| QueryError::General(error.to_string()))?
        {
            return Err(QueryError::General(format!(
                "foreign key constraint '{}' on '{}' still references '{}.{}'",
                constraint.field, child_table, table, reference_field
            )));
        }
    }

    Ok(())
}

fn assert_referenced_values_unchanged(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    before: &serde_json::Value,
    after: &serde_json::Value,
) -> Result<(), QueryError> {
    let (Some(before), Some(after)) = (before.as_object(), after.as_object()) else {
        return Ok(());
    };

    for (child_table, constraint) in referencing_constraints(cassie, table) {
        let Some(reference_field) = constraint.references_field.as_deref() else {
            continue;
        };
        let old_value = before.get(reference_field);
        let new_value = after.get(reference_field);
        if old_value == new_value {
            continue;
        }
        let Some(old_value) = old_value else {
            continue;
        };
        if old_value.is_null() {
            continue;
        }
        if cassie
            .value_exists_for_collection_field(
                session,
                &child_table,
                &constraint.field,
                old_value,
                None,
            )
            .map_err(|error| QueryError::General(error.to_string()))?
        {
            return Err(QueryError::General(format!(
                "foreign key constraint '{}' on '{}' still references '{}.{}'",
                constraint.field, child_table, table, reference_field
            )));
        }
    }

    Ok(())
}

pub(super) fn execute_update(
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
        assert_referenced_values_unchanged(
            cassie,
            session,
            &statement.table,
            &current.payload,
            &payload,
        )?;
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

pub(super) fn execute_delete(
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
        let current = cassie
            .get_document_for_session(session, &statement.table, &row_id)
            .map_err(|error| QueryError::General(error.to_string()))?
            .ok_or_else(|| {
                QueryError::General(format!(
                    "row '{row_id}' was not found in '{}'",
                    statement.table
                ))
            })?;
        assert_no_referencing_rows(cassie, session, &statement.table, &current.payload)?;
        if !statement.returning.is_empty() {
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
