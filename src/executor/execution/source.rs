use super::plan_inspection;
use super::*;

#[path = "source_join.rs"]
mod source_join;

type SourceExecution<'a> = Result<(Vec<Batch>, Vec<String>), QueryError>;

pub(super) struct SourceExecutionEnv<'a> {
    pub(super) cassie: &'a Cassie,
    pub(super) session: Option<&'a CassieSession>,
    pub(super) user_functions: &'a HashMap<String, FunctionMeta>,
    pub(super) params: &'a [Value],
    pub(super) controls: &'a QueryExecutionControls,
}

pub(super) fn execute_query_source<'a>(
    env: &'a SourceExecutionEnv<'a>,
    source: &'a QuerySource,
    cte_context: &'a mut CteContext,
    qualify: bool,
    outer_row: Option<&'a BatchRow>,
) -> SourceExecution<'a> {
    match source {
        QuerySource::Collection(name) => {
            if let Some(rows) = virtual_views::rows(&env.cassie.catalog, name) {
                let mut batches = materialize_virtual_rows(rows);
                if qualify {
                    batches = qualify_batches(batches, name);
                }
                ensure_temp_budget(env.controls, &batches)?;
                return Ok((batches, Vec::new()));
            }

            if let Some(view) = env.cassie.catalog.get_view(name) {
                let parsed = crate::sql::parser::parse_statement(&view.query)
                    .map_err(|error| QueryError::General(error.0))?;
                let logical = build_logical_plan(&parsed)?;
                let mut view_cte_context = CteContext::new();
                let rows = execute_plan(
                    env.cassie,
                    env.session,
                    &logical,
                    &mut view_cte_context,
                    env.user_functions,
                    env.params,
                    env.controls,
                )?;
                let rows = project_rows_to_schema(rows, &view.schema, name)?;
                let mut batches = batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE);
                if qualify {
                    batches = qualify_batches(batches, name);
                }
                ensure_temp_budget(env.controls, &batches)?;
                let text_fields = view
                    .schema
                    .fields
                    .iter()
                    .filter(|field| field.data_type == DataType::Text)
                    .map(|field| field.name.clone())
                    .collect::<Vec<_>>();
                return Ok((batches, text_fields));
            }

            if let Some(projection) = env.cassie.catalog.get_materialized_projection(name) {
                let output_collection = projection
                    .active_output_collection()
                    .ok_or_else(|| {
                        QueryError::General(format!(
                            "materialized projection '{name}' has no active version"
                        ))
                    })?
                    .to_string();
                let mut batches = scan::scan(env.cassie, env.session, &output_collection)?;
                if qualify {
                    batches = qualify_batches(batches, name);
                }
                ensure_temp_budget(env.controls, &batches)?;
                let text_fields = projection
                    .materialized
                    .map(|materialized| {
                        materialized
                            .output_schema
                            .fields
                            .into_iter()
                            .filter(|field| field.data_type == DataType::Text)
                            .map(|field| field.name)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                return Ok((batches, text_fields));
            }

            let mut batches = scan::scan(env.cassie, env.session, name)?;
            if qualify {
                batches = qualify_batches(batches, name);
            }
            ensure_temp_budget(env.controls, &batches)?;
            Ok((batches, env.cassie.catalog.text_fields(name)))
        }
        QuerySource::SingleRow => {
            let batches =
                batch::chunk_rows(vec![BatchRow::new(Vec::new())], batch::DEFAULT_BATCH_SIZE);
            ensure_temp_budget(env.controls, &batches)?;
            Ok((batches, Vec::new()))
        }
        QuerySource::Cte(name) => {
            let key = name.to_ascii_lowercase();
            let rows = cte_context
                .get(&key)
                .cloned()
                .ok_or_else(|| QueryError::General(format!("relation '{name}' does not exist")))?;
            let text_fields = deduce_text_fields(&rows);
            let mut batches = batch::chunk_rows(
                rows.into_iter().map(BatchRow::new).collect::<Vec<_>>(),
                batch::DEFAULT_BATCH_SIZE,
            );
            if qualify {
                batches = qualify_batches(batches, name);
            }
            ensure_temp_budget(env.controls, &batches)?;
            Ok((batches, text_fields))
        }
        QuerySource::Subquery {
            alias,
            select,
            lateral,
        } => {
            let logical = LogicalPlan {
                command: None,
                source: select.source.clone(),
                collection: alias.clone(),
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
            let mut subquery_context = cte_context.clone();
            let rows = execute_plan_with_outer_row(
                env.cassie,
                env.session,
                &logical,
                &mut subquery_context,
                env.user_functions,
                env.params,
                env.controls,
                if *lateral { outer_row } else { None },
            )?;
            let text_fields = deduce_text_fields(
                &rows
                    .iter()
                    .map(|row| row.entries().to_vec())
                    .collect::<Vec<_>>(),
            );
            let batches =
                qualify_batches(batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE), alias);
            ensure_temp_budget(env.controls, &batches)?;
            Ok((batches, text_fields))
        }
        QuerySource::Join {
            left,
            right,
            kind,
            on,
        } => {
            let (batches, text_fields) = source_join::execute_join_source(
                env,
                left,
                right,
                kind,
                on,
                cte_context,
                outer_row,
            )?;
            ensure_temp_budget(env.controls, &batches)?;
            Ok((batches, text_fields))
        }
    }
}

fn qualify_batches(batches: Vec<Batch>, qualifier: &str) -> Vec<Batch> {
    batches
        .into_iter()
        .map(|batch| {
            batch
                .into_iter()
                .map(|row| qualify_row(row, qualifier))
                .collect()
        })
        .collect()
}

fn combine_batches_with_outer_row(batches: Vec<Batch>, outer_row: &BatchRow) -> Vec<Batch> {
    batches
        .into_iter()
        .map(|batch| {
            batch
                .into_iter()
                .map(|row| combine_rows(outer_row, &row))
                .collect()
        })
        .collect()
}

fn source_contains_lateral(source: &QuerySource) -> bool {
    match source {
        QuerySource::Subquery { lateral, .. } => *lateral,
        QuerySource::Join { left, right, .. } => {
            source_contains_lateral(left) || source_contains_lateral(right)
        }
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => false,
    }
}

fn qualify_row(row: BatchRow, qualifier: &str) -> BatchRow {
    let qualifier = qualifier.to_ascii_lowercase();
    let mut values = Vec::new();
    for (name, value) in row.into_entries() {
        values.push((name.clone(), value.clone()));
        values.push((format!("{qualifier}.{name}"), value));
    }
    BatchRow::new(values)
}

fn combine_rows(left: &BatchRow, right: &BatchRow) -> BatchRow {
    let mut values = left.entries().to_vec();
    values.extend(right.entries().iter().cloned());
    BatchRow::new(values)
}

fn combine_row_with_nulls(left: &BatchRow, right_columns: &[String]) -> BatchRow {
    let mut values = left.entries().to_vec();
    values.extend(
        right_columns
            .iter()
            .map(|column| (column.clone(), Value::Null)),
    );
    BatchRow::new(values)
}

fn combine_nulls_with_row(left_columns: &[String], right: &BatchRow) -> BatchRow {
    let mut values = left_columns
        .iter()
        .map(|column| (column.clone(), Value::Null))
        .collect::<Vec<_>>();
    values.extend(right.entries().iter().cloned());
    BatchRow::new(values)
}

fn row_columns(rows: &[BatchRow]) -> Vec<String> {
    let mut columns = Vec::new();
    for row in rows {
        for (column, _) in row.entries() {
            if !columns.contains(column) {
                columns.push(column.clone());
            }
        }
    }
    columns
}

fn materialize_virtual_rows(rows: Vec<virtual_views::VirtualRow>) -> Vec<Batch> {
    batch::chunk_rows(
        rows.into_iter().map(BatchRow::new).collect::<Vec<_>>(),
        batch::DEFAULT_BATCH_SIZE,
    )
}

fn project_rows_to_schema(
    rows: Vec<BatchRow>,
    schema: &Schema,
    relation: &str,
) -> Result<Vec<BatchRow>, QueryError> {
    let mut projected = Vec::with_capacity(rows.len());
    for row in rows {
        let entries = row.into_entries();
        if entries.len() < schema.fields.len() {
            return Err(QueryError::General(format!(
                "view '{}' produced {} columns but schema expects {}",
                relation,
                entries.len(),
                schema.fields.len()
            )));
        }

        let mut values = Vec::with_capacity(schema.fields.len());
        for (field, (_name, value)) in schema.fields.iter().zip(entries) {
            values.push((field.name.clone(), value));
        }
        projected.push(BatchRow::new(values));
    }
    Ok(projected)
}

fn distinct_batches(batches: Vec<Batch>) -> Vec<Batch> {
    let mut rows = BTreeMap::<String, BatchRow>::new();
    for row in batch::flatten_batches(batches) {
        rows.entry(row_signature(&row)).or_insert(row);
    }
    batch::chunk_rows(rows.into_values().collect(), batch::DEFAULT_BATCH_SIZE)
}

fn distinct_on_batches(
    batches: Vec<Batch>,
    distinct_on: &[Expr],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Vec<Batch>, QueryError> {
    let mut seen = HashSet::<String>::new();
    let mut rows = Vec::new();
    for row in batch::flatten_batches(batches) {
        let key = distinct_on
            .iter()
            .map(|expr| {
                filter::evaluate_expr_value(
                    &row,
                    expr,
                    params,
                    search_context,
                    user_functions,
                    session,
                    None,
                )
                .map(|value| value_sort_key(&value))
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("|");
        if seen.insert(key) {
            rows.push(row);
        }
    }
    Ok(batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE))
}

fn apply_set_operation(
    left: Vec<BatchRow>,
    right: Vec<BatchRow>,
    set: &SelectSet,
) -> Result<Vec<BatchRow>, QueryError> {
    validate_set_width(&left, &right)?;
    match set.operator {
        SetOperator::UnionAll => {
            let mut rows = left;
            rows.extend(right);
            rows.sort_by_key(row_signature);
            Ok(rows)
        }
        SetOperator::Union => {
            let mut rows = left;
            rows.extend(right);
            let mut unique = BTreeMap::<String, BatchRow>::new();
            for row in rows {
                unique.entry(row_signature(&row)).or_insert(row);
            }
            Ok(unique.into_values().collect())
        }
        SetOperator::Intersect => {
            let right_signatures = right.iter().map(row_signature).collect::<HashSet<_>>();
            let mut unique = BTreeMap::<String, BatchRow>::new();
            for row in left {
                let signature = row_signature(&row);
                if right_signatures.contains(&signature) {
                    unique.entry(signature).or_insert(row);
                }
            }
            Ok(unique.into_values().collect())
        }
        SetOperator::Except => {
            let right_signatures = right.iter().map(row_signature).collect::<HashSet<_>>();
            let mut unique = BTreeMap::<String, BatchRow>::new();
            for row in left {
                let signature = row_signature(&row);
                if !right_signatures.contains(&signature) {
                    unique.entry(signature).or_insert(row);
                }
            }
            Ok(unique.into_values().collect())
        }
    }
}

pub(super) fn slice_rows(
    rows: Vec<BatchRow>,
    offset: Option<i64>,
    limit: Option<i64>,
) -> Vec<BatchRow> {
    let offset = offset
        .and_then(|value| usize::try_from(value.max(0)).ok())
        .unwrap_or(0);
    let limit = limit.and_then(|value| usize::try_from(value.max(0)).ok());
    let iter = rows.into_iter().skip(offset);
    match limit {
        Some(limit) => iter.take(limit).collect(),
        None => iter.collect(),
    }
}

fn validate_set_width(left: &[BatchRow], right: &[BatchRow]) -> Result<(), QueryError> {
    let left_width = left.first().map(|row| row.entries().len());
    let right_width = right.first().map(|row| row.entries().len());
    if let (Some(left_width), Some(right_width)) = (left_width, right_width) {
        if left_width != right_width {
            return Err(QueryError::General(format!(
                "set operation column count mismatch: {left_width} != {right_width}"
            )));
        }
    }
    Ok(())
}

fn plan_uses_aggregate(plan: &LogicalPlan) -> bool {
    !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.projection.iter().any(|item| match item {
            SelectItem::Function { function, .. } => {
                crate::sql::functions::is_aggregate_function(&function.name)
            }
            SelectItem::Wildcard
            | SelectItem::Column { .. }
            | SelectItem::Expr { .. }
            | SelectItem::WindowFunction { .. } => false,
        })
}

pub(crate) fn group_expr_name(expr: &Expr) -> String {
    match expr {
        Expr::Column(name) => name.clone(),
        _ => expr_key(expr),
    }
}

pub(crate) fn aggregate_signature(function: &FunctionCall) -> String {
    format!(
        "{}({})",
        function.name.to_ascii_lowercase(),
        function
            .args
            .iter()
            .map(expr_key)
            .collect::<Vec<_>>()
            .join(",")
    )
}

pub(crate) fn expr_key(expr: &Expr) -> String {
    match expr {
        Expr::Column(name) => name.clone(),
        Expr::Param(index) => format!("${}", index + 1),
        Expr::Null => "null".to_string(),
        Expr::BoolLiteral(value) => value.to_string(),
        Expr::NumberLiteral(value) => value.to_string(),
        Expr::StringLiteral(value) => format!("'{value}'"),
        Expr::Function(function) => aggregate_signature(function),
        Expr::Binary { left, op, right } => {
            format!("{}{:?}{}", expr_key(left), op, expr_key(right))
        }
        Expr::IsNull { expr, negated } => {
            format!(
                "{} is{} null",
                expr_key(expr),
                if *negated { " not" } else { "" }
            )
        }
        Expr::InList {
            expr,
            values,
            negated,
        } => format!(
            "{}{} in ({})",
            expr_key(expr),
            if *negated { " not" } else { "" },
            values.iter().map(expr_key).collect::<Vec<_>>().join(",")
        ),
        Expr::Between {
            expr,
            low,
            high,
            negated,
        } => format!(
            "{}{} between {} and {}",
            expr_key(expr),
            if *negated { " not" } else { "" },
            expr_key(low),
            expr_key(high)
        ),
        Expr::Not { expr } => format!("not {}", expr_key(expr)),
        Expr::Cast { expr, data_type } => format!("{}::{data_type:?}", expr_key(expr)),
        Expr::Exists(_) => "exists".to_string(),
    }
}

pub(crate) fn value_sort_key(value: &Value) -> String {
    match value {
        Value::Null => "0:null".to_string(),
        Value::Bool(value) => format!("1:{value}"),
        Value::Int64(value) => format!("2:{value:020}"),
        Value::Float64(value) => format!("3:{value:020.12}"),
        Value::String(value) => format!("4:{value}"),
        Value::Vector(value) => format!("5:{:?}", value.values),
        Value::Json(value) => format!("6:{value}"),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn execute_source_query_with_outer_row(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
    outer_row: Option<&BatchRow>,
) -> Result<Vec<BatchRow>, QueryError> {
    check_timeout(controls)?;
    let started_at = Instant::now();
    if let Some(rows) = aggregate_accel::try_execute_column_batch_aggregate(cassie, session, plan)?
    {
        return Ok(rows);
    }
    let env = SourceExecutionEnv {
        cassie,
        session,
        user_functions,
        params,
        controls,
    };
    let (mut batches, text_fields) =
        execute_query_source(&env, &plan.source, cte_context, false, outer_row)?;
    if let Some(outer_row) = outer_row {
        batches = combine_batches_with_outer_row(batches, outer_row);
    }
    let candidate_rows = batches.iter().map(|batch| batch.len()).sum::<usize>();

    let fulltext_fields = plan_inspection::fulltext_query_fields(plan);
    let uses_hybrid = plan_inspection::plan_uses_function(plan, "hybrid_score");
    let uses_vector = plan_inspection::plan_uses_vector_operator(plan);
    let search_context = if fulltext_fields.is_empty() {
        None
    } else {
        let (field_boost, field_k1, field_b, field_analyzer) =
            if let QuerySource::Collection(name) = &plan.source {
                let fields = cassie.catalog.text_fields(name);
                let mut boost = HashMap::with_capacity(fields.len());
                for field in fields {
                    if let Some(value) = cassie.catalog.get_field_boost(name, &field) {
                        boost.insert(field, value as f64);
                    }
                }

                let index_options = load_fulltext_index_options(cassie, name, &fulltext_fields)?;
                for (field, value) in index_options.field_boost {
                    boost.insert(field, value);
                }

                (
                    boost,
                    index_options.field_k1,
                    index_options.field_b,
                    index_options.field_analyzer,
                )
            } else {
                (
                    HashMap::new(),
                    HashMap::new(),
                    HashMap::new(),
                    HashMap::new(),
                )
            };

        Some(filter::SearchContext::from_rows(
            batches.iter().flat_map(|batch| batch.iter()),
            &text_fields,
            &field_boost,
            &field_k1,
            &field_b,
            &field_analyzer,
        ))
    };

    let resolved_filter = if let Some(filter_expr) = &plan.filter {
        Some(resolve_exists_expr(
            cassie,
            session,
            filter_expr,
            cte_context,
            user_functions,
            params,
            controls,
        )?)
    } else {
        None
    };

    if let Some(filter_expr) = &resolved_filter {
        batches = filter::filter_batches(
            batches,
            filter_expr,
            params,
            search_context.as_ref(),
            user_functions,
            session,
        )?;
        ensure_temp_budget(controls, &batches)?;
    }

    if plan_uses_aggregate(plan) {
        batches = aggregate_exec::aggregate_query_batches(
            cassie,
            batches,
            plan,
            params,
            search_context.as_ref(),
            user_functions,
            session,
            controls,
        )?;
        ensure_temp_budget(controls, &batches)?;
        if let Some(having) = &plan.having {
            let having = aggregate_exec::rewrite_aggregate_expr(having);
            batches = filter::filter_batches(
                batches,
                &having,
                params,
                search_context.as_ref(),
                user_functions,
                session,
            )?;
            ensure_temp_budget(controls, &batches)?;
        }
    }

    batches = window_exec::apply_window_functions(
        batches,
        &plan.projection,
        params,
        search_context.as_ref(),
        user_functions,
        session,
    )?;
    ensure_temp_budget(controls, &batches)?;

    if !plan.distinct_on.is_empty() {
        batches = sort::sort_batches(
            batches,
            &plan.order,
            &plan.projection,
            params,
            search_context.as_ref(),
            user_functions,
            session,
        )?;
        ensure_temp_budget(controls, &batches)?;
        batches = distinct_on_batches(
            batches,
            &plan.distinct_on,
            params,
            search_context.as_ref(),
            user_functions,
            session,
        )?;
        ensure_temp_budget(controls, &batches)?;
    } else if plan.set.is_none() && !plan.order.is_empty() {
        batches = sort::sort_batches(
            batches,
            &plan.order,
            &plan.projection,
            params,
            search_context.as_ref(),
            user_functions,
            session,
        )?;
        ensure_temp_budget(controls, &batches)?;
    }

    batches = projection::project_batches(
        batches,
        &plan.projection,
        params,
        search_context.as_ref(),
        user_functions,
        session,
    )?;
    ensure_temp_budget(controls, &batches)?;

    if plan.distinct {
        batches = distinct_batches(batches);
        ensure_temp_budget(controls, &batches)?;
    }

    let mut rows = batch::flatten_batches(batches);
    if let Some(set) = &plan.set {
        let right_plan = plan_inspection::logical_plan_from_select(&set.right);
        let right_rows = execute_plan(
            cassie,
            session,
            &right_plan,
            cte_context,
            user_functions,
            params,
            controls,
        )?;
        rows = apply_set_operation(rows, right_rows, set)?;
    }
    if plan.set.is_some() && !plan.order.is_empty() {
        rows = sort::sort_rows(
            rows,
            &plan.order,
            &plan.projection,
            params,
            search_context.as_ref(),
            user_functions,
            session,
        )?;
    }
    rows = slice_rows(rows, plan.offset, plan.limit);

    let elapsed = started_at.elapsed();
    if !fulltext_fields.is_empty() {
        cassie
            .runtime
            .record_search_execution(elapsed, candidate_rows, rows.len());
    }
    if uses_hybrid {
        cassie
            .runtime
            .record_hybrid_execution(elapsed, candidate_rows, rows.len());
    }
    if uses_vector {
        cassie
            .runtime
            .record_vector_execution(elapsed, candidate_rows, rows.len());
    }

    Ok(rows)
}
