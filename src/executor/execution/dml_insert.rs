#[path = "insert_wait_inputs.rs"]
mod wait_inputs;
use super::dml_returning;
use super::{
    apply_update_value, build_dml_result, check_timeout, dml_referential_actions, execute_plan,
    filter, inserted_row_to_batch_row, integral_json_number, json_to_value,
    value_to_json_for_field, BatchRow, Cassie, CassieSession, CollectionSchema, CteContext,
    DataType, DmlResultContext, Expr, FieldMeta, FunctionMeta, HashMap, InsertSource, LogicalPlan,
    QueryError, QueryExecutionControls, QueryResult, QuerySource, Value,
};
pub(in crate::executor::execution) use wait_inputs::{
    execute_prepared_insert, prepare_waiting_insert, WaitingInsert,
};

#[path = "insert_cells.rs"]
mod cells;
#[path = "dml_insert_conflict.rs"]
mod conflict;
#[path = "dml_insert_conflict_update.rs"]
mod conflict_update;
#[path = "insert_values.rs"]
mod scalar_values;

use conflict::find_insert_conflict_row_id;
use conflict_update::{
    execute_insert_conflict_update, reject_second_conflict_update,
    resolve_autocommit_insert_conflict,
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
    let (has_exists_returning, returning_read_controls) =
        prepare_insert_returning_read_controls(cassie, session, &statement.returning, controls)?;
    let statement_read_controls = returning_read_controls.as_ref().unwrap_or(controls);
    let schema = cassie.catalog.get_schema(&statement.table).ok_or_else(|| {
        QueryError::General(format!("collection '{}' not found", statement.table))
    })?;
    let (target_fields, source_rows) = prepare_insert_source_rows(
        cassie,
        session,
        statement,
        params,
        user_functions,
        &schema,
        statement_read_controls,
    )?;

    let payloads = source_rows
        .into_iter()
        .map(|row| cells::payload(cassie, &statement.table, &target_fields, &row));
    execute_insert_payloads(
        cassie,
        session,
        statement,
        params,
        user_functions,
        controls,
        InsertPayloads {
            schema,
            has_exists_returning,
            returning_read_controls,
            payloads,
        },
    )
}

struct InsertPayloads<I> {
    schema: CollectionSchema,
    has_exists_returning: bool,
    returning_read_controls: Option<QueryExecutionControls>,
    payloads: I,
}

fn execute_insert_payloads(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::InsertStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
    inputs: InsertPayloads<impl Iterator<Item = Result<serde_json::Value, QueryError>>>,
) -> Result<QueryResult, QueryError> {
    let InsertPayloads {
        schema,
        has_exists_returning,
        returning_read_controls,
        payloads,
    } = inputs;
    let statement_read_controls = returning_read_controls.as_ref().unwrap_or(controls);
    let mut affected_count = 0usize;
    let mut returning_rows = Vec::new();
    let conflict_images = std::cell::RefCell::new(HashMap::new());
    let insert_context = InsertExecutionContext {
        cassie,
        session,
        statement,
        params,
        user_functions,
        schema: &schema,
        controls,
        apply_defaults: false,
        conflict_images: statement
            .returning_aliases
            .is_some()
            .then_some(&conflict_images),
    };
    let returning_schema = row_image_schema(statement, &schema);
    let result_context = DmlResultContext {
        cassie,
        session,
        table: &statement.table,
        returning: &statement.returning,
        params,
        user_functions,
        command_prefix: "INSERT 0",
        controls,
        statement_read_controls,
        returning_schema: returning_schema.as_ref(),
    };
    let projection_schema = has_exists_returning
        .then(|| dml_returning::prepare_projection_schema(&result_context))
        .transpose()?;
    let mut projected_returning_rows = projection_schema
        .as_ref()
        .map(|_| crate::runtime::accounted::AccountedVec::try_new(controls))
        .transpose()?;
    let mut affected_row_ids = std::collections::HashSet::new();
    for payload in payloads {
        check_timeout(controls)?;
        let Some(row_id) = execute_insert_payload(&insert_context, payload?, &affected_row_ids)?
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
        attach_insert_returning_aliases(
            statement,
            &schema,
            &row_id,
            &conflict_images,
            &mut returning_rows,
        );
        project_insert_returning_row(
            &result_context,
            projection_schema.as_ref(),
            projected_returning_rows.as_mut(),
            &mut returning_rows,
        )?;
    }
    finish_insert_returning(
        &result_context,
        projection_schema.as_ref(),
        projected_returning_rows,
        affected_count,
        returning_rows,
    )
}

fn prepare_insert_returning_read_controls(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    returning: &[crate::sql::ast::SelectItem],
    controls: &QueryExecutionControls,
) -> Result<(bool, Option<QueryExecutionControls>), QueryError> {
    let has_exists_returning = crate::executor::execution::exists_projection::contains(returning);
    let returning_read_controls = if has_exists_returning {
        Some(
            crate::executor::execution::entrypoints::statement_read_controls(
                cassie, session, controls,
            )?,
        )
    } else {
        None
    };
    Ok((has_exists_returning, returning_read_controls))
}

fn prepare_insert_source_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::InsertStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    schema: &CollectionSchema,
    controls: &QueryExecutionControls,
) -> Result<(Vec<FieldMeta>, Vec<Vec<cells::InsertValue>>), QueryError> {
    let source_rows =
        insert_source_rows(cassie, session, statement, params, user_functions, controls)?;
    let source_width = source_rows
        .first()
        .map_or_else(|| insert_source_width(statement, schema), Vec::len);
    let target_fields = insert_target_fields(statement, schema, source_width)?;
    validate_insert_source_rows(&source_rows, target_fields.len())?;
    super::identity_insert::apply_identity_policy(
        cassie,
        statement,
        target_fields,
        source_rows,
        |cell| matches!(cell, cells::InsertValue::Default),
    )
}

fn attach_insert_returning_aliases(
    statement: &crate::sql::ast::InsertStatement,
    schema: &CollectionSchema,
    row_id: &str,
    conflict_images: &std::cell::RefCell<HashMap<String, serde_json::Value>>,
    returning_rows: &mut Vec<BatchRow>,
) {
    if let Some(aliases) = &statement.returning_aliases {
        if let Some(row) = returning_rows.pop() {
            let before = conflict_images.borrow_mut().remove(row_id);
            returning_rows.push(with_row_images(row, schema, aliases, before.as_ref()));
        }
    }
}

fn project_insert_returning_row(
    context: &DmlResultContext<'_>,
    projection_schema: Option<&dml_returning::ProjectionSchema>,
    projected_rows: Option<&mut crate::runtime::accounted::AccountedVec<BatchRow>>,
    returning_rows: &mut Vec<BatchRow>,
) -> Result<(), QueryError> {
    if let (Some(projection_schema), Some(projected_rows)) = (projection_schema, projected_rows) {
        if let Some(row) = returning_rows.pop() {
            let projected = dml_returning::project(context, projection_schema, vec![row])?;
            for row in projected {
                projected_rows.try_push_with(0, || row)?;
            }
        }
    }
    Ok(())
}

fn finish_insert_returning(
    context: &DmlResultContext<'_>,
    projection_schema: Option<&dml_returning::ProjectionSchema>,
    projected_rows: Option<crate::runtime::accounted::AccountedVec<BatchRow>>,
    affected_count: usize,
    returning_rows: Vec<BatchRow>,
) -> Result<QueryResult, QueryError> {
    if let (Some(projection_schema), Some(projected_rows)) = (projection_schema, projected_rows) {
        let (rows, _rows_memory) = projected_rows.into_parts();
        dml_returning::finish_projected(context, projection_schema, affected_count, rows)
    } else {
        build_dml_result(context, affected_count, returning_rows)
    }
}

/// Inserts one already-evaluated row, as `MERGE ... THEN INSERT` does, and
/// returns its stored row id.
pub(super) fn insert_evaluated_row(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::InsertStatement,
    row: Vec<Value>,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<String, QueryError> {
    check_timeout(controls)?;
    let schema = cassie.catalog.get_schema(&statement.table).ok_or_else(|| {
        QueryError::General(format!("collection '{}' not found", statement.table))
    })?;
    let target_fields = insert_target_fields(statement, &schema, row.len())?;
    let (target_fields, rows) = super::identity_insert::apply_identity_policy(
        cassie,
        statement,
        target_fields,
        vec![row],
        |_| false,
    )?;
    let context = InsertExecutionContext {
        cassie,
        session,
        statement,
        params,
        user_functions,
        schema: &schema,
        controls,
        apply_defaults: true,
        conflict_images: None,
    };
    let row = rows.into_iter().next().unwrap_or_default();
    execute_insert_source_row(
        &context,
        &target_fields,
        &row,
        &std::collections::HashSet::new(),
    )?
    .ok_or_else(|| QueryError::General("INSERT stored no row".to_string()))
}

fn insert_source_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::InsertStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Vec<cells::InsertValue>>, QueryError> {
    match &statement.source {
        InsertSource::Values(rows) => scalar_values::evaluate(
            rows,
            &scalar_values::ValuesContext {
                cassie,
                params,
                functions: user_functions,
                session,
                controls,
            },
        ),
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
                    QuerySource::Values { .. } => "values".into(),
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
                        .map(|(_, value)| cells::InsertValue::Value(value))
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
        if matches!(field.data_type, DataType::Json | DataType::Jsonb)
            && matches!(value, Value::Null)
        {
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
    source_rows: &[Vec<cells::InsertValue>],
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
    apply_defaults: bool,
    /// Prior payloads of rows a conflict update changed, for `RETURNING OLD`.
    conflict_images: Option<&'a std::cell::RefCell<HashMap<String, serde_json::Value>>>,
}

fn execute_insert_source_row(
    context: &InsertExecutionContext<'_>,
    target_fields: &[FieldMeta],
    source_row: &[Value],
    affected_row_ids: &std::collections::HashSet<String>,
) -> Result<Option<String>, QueryError> {
    let payload = serde_json::Value::Object(payload_from_insert_row(target_fields, source_row)?);
    execute_insert_payload(context, payload, affected_row_ids)
}

fn execute_insert_payload(
    context: &InsertExecutionContext<'_>,
    payload: serde_json::Value,
    affected_row_ids: &std::collections::HashSet<String>,
) -> Result<Option<String>, QueryError> {
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
                ..
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
            context.apply_defaults,
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
            apply_defaults: false,
            conflict_images: None,
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
            ..
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

/// The `RETURNING` row shape when it names `OLD`/`NEW` row images.
fn row_image_schema(
    statement: &crate::sql::ast::InsertStatement,
    schema: &CollectionSchema,
) -> Option<CollectionSchema> {
    statement.returning_aliases.as_ref().map(|aliases| {
        super::dml_joined::joined_returning_schema(
            schema,
            &statement
                .alias
                .clone()
                .unwrap_or_else(|| crate::catalog::local_name(&statement.table)),
            &[],
            Some(aliases),
            false,
        )
    })
}

/// Adds the `OLD`/`NEW` row images to an `INSERT ... RETURNING` row; `OLD` is
/// the conflicting row's prior state, or NULL for an inserted row.
fn with_row_images(
    row: BatchRow,
    schema: &CollectionSchema,
    aliases: &crate::sql::ast::ReturningAliases,
    before: Option<&serde_json::Value>,
) -> BatchRow {
    let after = serde_json::Value::Object(
        schema
            .fields
            .iter()
            .filter_map(|field| {
                row.get(&field.name)
                    .and_then(|value| super::value_to_json(value).ok())
                    .map(|value| (field.name.clone(), value))
            })
            .collect(),
    );
    let (mut values, aliases_index) = row.into_parts();
    values.extend(super::dml_joined::row_image_entries(
        schema,
        Some(aliases),
        before,
        Some(&after),
    ));
    BatchRow::with_aliases(values, aliases_index)
}

fn insert_target_fields(
    statement: &crate::sql::ast::InsertStatement,
    schema: &CollectionSchema,
    value_count: usize,
) -> Result<Vec<FieldMeta>, QueryError> {
    if statement.columns.is_empty() {
        // `DEFAULT VALUES` names no column and supplies no value.
        if value_count == 0 {
            return Ok(Vec::new());
        }
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
        Expr::NumericLiteral(_)
        | Expr::Column(_)
        | Expr::Case { .. }
        | Expr::Function(_)
        | Expr::IsNull { .. }
        | Expr::TruthTest { .. }
        | Expr::InList { .. }
        | Expr::Between { .. }
        | Expr::Like { .. }
        | Expr::Not { .. }
        | Expr::RowCompare(_)
        | Expr::UnaryNumeric { .. }
        | Expr::IntegerNegate { .. }
        | Expr::Cast { .. }
        | Expr::Exists(_)
        | Expr::Subquery(_)
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
