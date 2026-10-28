use super::{
    apply_update_value, batch, build_dml_result, check_timeout, dml_referential_actions,
    ensure_query_memory_budget, filter, inserted_row_to_batch_row, row_id_from_batch_row, scan,
    BatchRow, Cassie, CassieSession, CollectionSchema, DmlResultContext, Expr, FunctionMeta,
    HashMap, QueryError, QueryExecutionControls, QueryResult, Value,
};

struct PreparedUpdateRow {
    row_id: String,
    before_payload: serde_json::Value,
    payload: serde_json::Value,
}

struct PrepareUpdateContext<'a> {
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    statement: &'a crate::sql::ast::UpdateStatement,
    params: &'a [Value],
    user_functions: &'a HashMap<String, FunctionMeta>,
    schema: &'a CollectionSchema,
    controls: &'a QueryExecutionControls,
    subqueries: &'a super::dml_subqueries::RowSubqueries<'a>,
}

pub(in crate::executor::execution) fn execute_update(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::UpdateStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    check_timeout(controls)?;
    if super::dml_joined_update::update_is_joined(statement) {
        return super::dml_joined_update::execute_joined_update(
            cassie,
            session,
            statement,
            params,
            user_functions,
            controls,
        );
    }
    let returning_read_controls = if super::super::exists_projection::contains(&statement.returning)
        || statement
            .assignments
            .iter()
            .map(|(_, expr)| expr)
            .chain(&statement.filter)
            .any(Expr::contains_subquery)
    {
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
    let subqueries = super::dml_subqueries::RowSubqueries {
        cassie,
        session,
        user_functions,
        params,
        controls: statement_read_controls,
        table: &statement.table,
        alias: statement.alias.as_deref(),
    };
    let matched_rows = matched_dml_rows(
        cassie,
        session,
        &statement.table,
        statement.filter.as_ref(),
        &subqueries,
        statement_read_controls,
    )?;
    let mut folded = statement.clone();
    for (_, expr) in &mut folded.assignments {
        *expr = subqueries.fold(expr)?;
    }
    let statement = &folded;
    let mut returning_rows = Vec::new();
    let prepare_context = PrepareUpdateContext {
        cassie,
        session,
        statement,
        params,
        user_functions,
        schema: &schema,
        controls,
        subqueries: &subqueries,
    };
    for row in &matched_rows {
        check_timeout(controls)?;
        let prepared = prepare_update_row(&prepare_context, row)?;
        apply_update_row(
            cassie,
            session,
            statement,
            &schema,
            prepared,
            &mut returning_rows,
            controls,
        )?;
    }
    let updated_count = if statement.returning.is_empty() {
        matched_rows.len()
    } else {
        returning_rows.len()
    };
    build_dml_result(
        &DmlResultContext {
            cassie,
            session,
            table: &statement.table,
            returning: &statement.returning,
            params,
            user_functions,
            command_prefix: "UPDATE",
            controls,
            statement_read_controls,
            returning_schema: None,
        },
        updated_count,
        returning_rows,
    )
}

fn matched_dml_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    filter_expr: Option<&Expr>,
    subqueries: &super::dml_subqueries::RowSubqueries<'_>,
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, QueryError> {
    let statement_controls = crate::executor::execution::entrypoints::statement_read_controls(
        cassie, session, controls,
    )?;
    let controls = &statement_controls;
    let (_read_scope, _overlay_scope) =
        crate::executor::execution::entrypoints::enter_statement_read(controls);
    let batches = scan::scan(cassie, session, table, controls)?;
    ensure_query_memory_budget(controls, &batches)?;
    let rows = batch::flatten_batches(batches);
    if let Some(filter_expr) = filter_expr {
        subqueries.filter(rows, filter_expr)
    } else {
        Ok(rows)
    }
}

fn prepare_update_row(
    context: &PrepareUpdateContext<'_>,
    row: &BatchRow,
) -> Result<PreparedUpdateRow, QueryError> {
    check_timeout(context.controls)?;
    let row_id = row_id_from_batch_row(row)?;
    let current = context
        .cassie
        .get_document_for_session(context.session, &context.statement.table, &row_id)
        .map_err(QueryError::from)?
        .ok_or_else(|| {
            QueryError::General(format!(
                "row '{row_id}' was not found in '{}'",
                context.statement.table
            ))
        })?;
    let payload = updated_payload_from_row(
        &AssignmentTarget {
            cassie: context.cassie,
            table: &context.statement.table,
            schema: context.schema,
            subqueries: Some(context.subqueries),
            array_targets: &context.statement.array_targets,
        },
        row,
        &context.statement.assignments,
        context.params,
        context.user_functions,
        context.session,
        &current.payload,
    )?;
    let mut payload = payload;
    context.cassie.discard_stale_vector_embeddings(
        &context.statement.table,
        &mut payload,
        context
            .statement
            .assignments
            .iter()
            .map(|(field, _)| field.as_str()),
    );
    let payload = context
        .cassie
        .prepare_document_write_for_session(
            context.session,
            &context.statement.table,
            payload,
            false,
            Some(&row_id),
        )
        .map_err(QueryError::from)?;
    Ok(PreparedUpdateRow {
        row_id,
        before_payload: current.payload,
        payload,
    })
}

/// The relation an assignment list writes.
pub(super) struct AssignmentTarget<'a> {
    pub(super) cassie: &'a Cassie,
    pub(super) table: &'a str,
    pub(super) schema: &'a CollectionSchema,
    pub(super) subqueries: Option<&'a super::dml_subqueries::RowSubqueries<'a>>,
    pub(super) array_targets: &'a [crate::sql::ast::UpdateTarget],
}

pub(super) fn updated_payload_from_row(
    target: &AssignmentTarget<'_>,
    row: &BatchRow,
    assignments: &[(String, Expr)],
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    current_payload: &serde_json::Value,
) -> Result<serde_json::Value, QueryError> {
    let schema = target.schema;
    let mut payload = current_payload
        .as_object()
        .cloned()
        .ok_or_else(|| QueryError::General("stored row payload must be object".to_string()))?;
    let schema_has_id = schema.declares_id();
    for (assignment_index, (field, expr)) in assignments.iter().enumerate() {
        if expr.is_default_assignment() {
            match target
                .cassie
                .column_default_json(target.table, field)
                .map_err(QueryError::from)?
            {
                Some(value) => payload.insert(field.clone(), value),
                None => payload.remove(field),
            };
            continue;
        }
        let mut expr = expr.clone();
        crate::planner::logical::rewrite_expr_for_schema(&mut expr, schema_has_id);
        let value = match target.subqueries {
            Some(subqueries) if expr.contains_subquery() => subqueries.evaluate(row, &expr)?,
            _ => filter::evaluate_expr_value(
                row,
                &expr,
                params,
                None,
                user_functions,
                session,
                None,
            )?,
        };
        if let Some(array_target) = target
            .array_targets
            .iter()
            .find(|target| target.assignment_index == assignment_index)
        {
            apply_array_update_value(
                &mut payload,
                field,
                &array_target.subscripts,
                &value,
                (row, params, user_functions, session),
                schema,
            )?;
        } else {
            apply_update_value(&mut payload, field, &value, schema)?;
        }
    }
    Ok(serde_json::Value::Object(payload))
}

fn apply_array_update_value(
    payload: &mut serde_json::Map<String, serde_json::Value>,
    field: &str,
    subscripts: &[crate::sql::ast::ArraySubscript],
    assigned: &Value,
    evaluation: (
        &BatchRow,
        &[Value],
        &HashMap<String, FunctionMeta>,
        Option<&CassieSession>,
    ),
    schema: &CollectionSchema,
) -> Result<(), QueryError> {
    let (row, params, user_functions, session) = evaluation;
    let field_schema = schema
        .fields
        .iter()
        .find(|candidate| candidate.name == field)
        .ok_or_else(|| QueryError::General(format!("array UPDATE column '{field}' is missing")))?;
    let crate::types::DataType::Array(element_type) = &field_schema.data_type else {
        return Err(QueryError::General(format!(
            "array UPDATE target '{field}' is not an array"
        )));
    };
    if subscripts.len() != 1 {
        return Err(QueryError::General(
            "array UPDATE element and slice assignment currently requires one dimension".into(),
        ));
    }
    let mut array = match payload.get(field) {
        Some(value) if value.is_null() => crate::types::SqlArray::empty(),
        Some(value)
            if value
                .as_object()
                .is_some_and(|object| object.contains_key("$cassie_array_v1")) =>
        {
            crate::types::SqlArray::from_storage_json(value).map_err(QueryError::General)?
        }
        Some(value) => crate::types::SqlArray::from_nested_json(
            value,
            matches!(
                element_type.as_ref(),
                crate::types::DataType::Json | crate::types::DataType::Jsonb
            ),
        )
        .map_err(QueryError::General)?,
        None => crate::types::SqlArray::empty(),
    };
    match &subscripts[0] {
        crate::sql::ast::ArraySubscript::Index(expression) => {
            let index = evaluate_array_bound(expression, row, params, user_functions, session)?;
            set_array_element(&mut array, index, assigned.clone())?;
        }
        crate::sql::ast::ArraySubscript::Slice { lower, upper } => {
            let default_lower = array
                .dimensions()
                .first()
                .map_or(1, |dimension| dimension.lower_bound);
            let default_upper = array
                .dimensions()
                .first()
                .map_or(0, |dimension| dimension.lower_bound + dimension.length - 1);
            let lower = lower
                .as_ref()
                .map(|expression| {
                    evaluate_array_bound(expression, row, params, user_functions, session)
                })
                .transpose()?
                .unwrap_or(default_lower);
            let upper = upper
                .as_ref()
                .map(|expression| {
                    evaluate_array_bound(expression, row, params, user_functions, session)
                })
                .transpose()?
                .unwrap_or(default_upper);
            let replacement = match assigned {
                Value::Array(array) => array.clone(),
                Value::Json(value) if value.is_array() => crate::types::SqlArray::from_nested_json(
                    value,
                    matches!(
                        element_type.as_ref(),
                        crate::types::DataType::Json | crate::types::DataType::Jsonb
                    ),
                )
                .map_err(QueryError::General)?,
                _ => {
                    return Err(QueryError::General(
                        "array slice assignment requires an array value".into(),
                    ));
                }
            };
            set_array_slice(&mut array, lower, upper, &replacement)?;
        }
    }
    payload.insert(field.to_string(), array.to_storage_json());
    Ok(())
}

fn evaluate_array_bound(
    expression: &Expr,
    row: &BatchRow,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<i32, QueryError> {
    let value =
        filter::evaluate_expr_value(row, expression, params, None, user_functions, session, None)?;
    let index = value
        .as_i64()
        .and_then(|index| i32::try_from(index).ok())
        .ok_or_else(|| QueryError::General("array subscript must be an INT4 value".into()))?;
    Ok(index)
}

fn set_array_element(
    array: &mut crate::types::SqlArray,
    index: i32,
    value: Value,
) -> Result<(), QueryError> {
    if array.dimensions().is_empty() {
        *array = crate::types::SqlArray::new(
            vec![crate::types::ArrayDimension {
                length: 1,
                lower_bound: index,
            }],
            vec![value],
        )
        .map_err(QueryError::General)?;
        return Ok(());
    }
    if array.dimensions().len() != 1 {
        return Err(QueryError::General(
            "array element assignment requires a one-dimensional array".into(),
        ));
    }
    let dimension = array.dimensions()[0];
    let upper_bound = dimension
        .lower_bound
        .checked_add(dimension.length - 1)
        .ok_or_else(|| QueryError::General("array upper bound exceeds INT4".into()))?;
    let mut elements = array.elements().to_vec();
    let mut lower_bound = dimension.lower_bound;
    if index < lower_bound {
        let prefix = usize::try_from(
            lower_bound
                .checked_sub(index)
                .ok_or_else(|| QueryError::General("array subscript is out of range".into()))?,
        )
        .map_err(|_| QueryError::General("array dimension exceeds INT4".into()))?;
        let mut expanded = Vec::new();
        expanded
            .try_reserve(prefix.saturating_add(elements.len()))
            .map_err(|error| QueryError::General(format!("cannot expand array: {error}")))?;
        expanded.resize(prefix, Value::Null);
        expanded.append(&mut elements);
        elements = expanded;
        lower_bound = index;
    }
    let relative_index = index
        .checked_sub(lower_bound)
        .ok_or_else(|| QueryError::General("array subscript is out of range".into()))?;
    let position = usize::try_from(relative_index)
        .map_err(|_| QueryError::General("array subscript is out of range".into()))?;
    if index > upper_bound {
        let new_length = usize::try_from(
            index
                .checked_sub(lower_bound)
                .and_then(|relative| relative.checked_add(1))
                .ok_or_else(|| QueryError::General("array dimension exceeds INT4".into()))?,
        )
        .map_err(|_| QueryError::General("array dimension exceeds INT4".into()))?;
        elements
            .try_reserve(new_length.saturating_sub(elements.len()))
            .map_err(|error| QueryError::General(format!("cannot expand array: {error}")))?;
        elements.resize(new_length, Value::Null);
    }
    elements[position] = value;
    *array = crate::types::SqlArray::new(
        vec![crate::types::ArrayDimension {
            length: i32::try_from(elements.len())
                .map_err(|_| QueryError::General("array dimension exceeds INT4".into()))?,
            lower_bound,
        }],
        elements,
    )
    .map_err(QueryError::General)?;
    Ok(())
}

fn set_array_slice(
    array: &mut crate::types::SqlArray,
    lower: i32,
    upper: i32,
    replacement: &crate::types::SqlArray,
) -> Result<(), QueryError> {
    if replacement.dimensions().len() > 1 {
        return Err(QueryError::General(
            "array slice replacement must be one-dimensional".into(),
        ));
    }
    if upper < lower {
        return Ok(());
    }
    let (mut old_lower, mut old_length, mut elements) = if array.is_empty() {
        (1, 0_usize, Vec::new())
    } else {
        if array.dimensions().len() != 1 {
            return Err(QueryError::General(
                "array slice assignment requires a one-dimensional array".into(),
            ));
        }
        (
            array.dimensions()[0].lower_bound,
            usize::try_from(array.dimensions()[0].length)
                .map_err(|_| QueryError::General("array dimension out of range".into()))?,
            array.elements().to_vec(),
        )
    };
    if lower < old_lower {
        let prefix =
            usize::try_from(old_lower.checked_sub(lower).ok_or_else(|| {
                QueryError::General("array slice lower bound out of range".into())
            })?)
            .map_err(|_| QueryError::General("array slice lower bound out of range".into()))?;
        let mut expanded = Vec::new();
        expanded
            .try_reserve(prefix.saturating_add(elements.len()))
            .map_err(|error| QueryError::General(format!("cannot expand array slice: {error}")))?;
        expanded.resize(prefix, Value::Null);
        expanded.append(&mut elements);
        elements = expanded;
        old_length = old_length
            .checked_add(prefix)
            .ok_or_else(|| QueryError::General("array dimension exceeds INT4".into()))?;
        old_lower = lower;
    }
    let old_upper = old_lower
        .checked_add(
            i32::try_from(old_length)
                .map_err(|_| QueryError::General("array dimension exceeds INT4".into()))?,
        )
        .and_then(|exclusive| exclusive.checked_sub(1))
        .ok_or_else(|| QueryError::General("array upper bound exceeds INT4".into()))?;
    if upper > old_upper {
        let extra =
            usize::try_from(upper.checked_sub(old_upper).ok_or_else(|| {
                QueryError::General("array slice upper bound out of range".into())
            })?)
            .map_err(|_| QueryError::General("array slice upper bound out of range".into()))?;
        elements
            .try_reserve(extra)
            .map_err(|error| QueryError::General(format!("cannot expand array slice: {error}")))?;
        let new_length = elements
            .len()
            .checked_add(extra)
            .ok_or_else(|| QueryError::General("array dimension exceeds INT4".into()))?;
        elements.resize(new_length, Value::Null);
    }
    let start = usize::try_from(
        lower
            .checked_sub(old_lower)
            .ok_or_else(|| QueryError::General("array slice lower bound out of range".into()))?,
    )
    .map_err(|_| QueryError::General("array slice lower bound out of range".into()))?;
    let end = usize::try_from(
        upper
            .checked_sub(old_lower)
            .and_then(|relative| relative.checked_add(1))
            .ok_or_else(|| QueryError::General("array slice upper bound out of range".into()))?,
    )
    .map_err(|_| QueryError::General("array slice upper bound out of range".into()))?;
    elements
        .try_reserve(replacement.len())
        .map_err(|error| QueryError::General(format!("cannot assign array slice: {error}")))?;
    elements.splice(start..end, replacement.elements().iter().cloned());
    *array = crate::types::SqlArray::new(
        vec![crate::types::ArrayDimension {
            length: i32::try_from(elements.len())
                .map_err(|_| QueryError::General("array dimension exceeds INT4".into()))?,
            lower_bound: old_lower,
        }],
        elements,
    )
    .map_err(QueryError::General)?;
    Ok(())
}

fn apply_update_row(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::UpdateStatement,
    schema: &CollectionSchema,
    prepared: PreparedUpdateRow,
    returning_rows: &mut Vec<BatchRow>,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    check_timeout(controls)?;
    let row_id = prepared.row_id.clone();
    let document = dml_referential_actions::update_existing_row(
        cassie,
        session,
        &statement.table,
        &prepared.row_id,
        &prepared.before_payload,
        prepared.payload,
        controls,
    )?;
    if !statement.returning.is_empty() {
        returning_rows.push(inserted_row_to_batch_row(
            &row_id,
            schema,
            &document.payload,
        ));
    }
    Ok(())
}
