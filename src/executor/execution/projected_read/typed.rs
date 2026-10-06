use super::{
    point_lookup_read_spec, scan, virtual_views, BatchRow, Cassie, CassieSession,
    ExecutionBreakdownDurations, Expr, FunctionMeta, HashMap, Instant, LogicalPlan, QueryError,
    QueryExecutionControls, QuerySource, SelectItem, Value,
};
use crate::executor::retained_memory::{add, data_type_clone_bytes, mul};
use crate::executor::typed_batch::TypedBatch;
use std::mem::size_of;
use std::sync::Arc;

pub(super) fn try_execute_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    try_execute(cassie, session, plan, functions, params, controls)
        .map(|result| result.map(|(rows, _)| rows))
}

pub(super) fn try_execute(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Option<(Vec<BatchRow>, ExecutionBreakdownDurations)>, QueryError> {
    if !has_transport_budget(controls)? {
        return Ok(None);
    }
    if !supports_plan(plan) {
        return Ok(None);
    }
    let QuerySource::Collection(collection) = &plan.source else {
        return Ok(None);
    };
    if virtual_views::schema(collection).is_some()
        || !scan::uses_controlled_row_scan(cassie, collection)
        || cassie.catalog.get_view(collection).is_some()
        || point_lookup_read_spec(plan, params).is_some()
    {
        return Ok(None);
    }
    let (schema, _schema_memory) = cassie
        .catalog
        .clone_schema_with_controls(collection, controls)?;
    let Some(schema) = schema else {
        return Ok(None);
    };
    let Some((fields, _field_memory)) = scan_fields(plan, &schema, controls)? else {
        return Ok(None);
    };
    // CBC2 retains its pruning, bounded reads and all-or-nothing fallback boundary.
    if (plan.filter.is_some() || plan.limit.is_some() || plan.offset.is_some())
        && scan::has_covering_column_index(cassie, collection, &fields)
    {
        return Ok(None);
    }
    let mut breakdown = ExecutionBreakdownDurations::default();
    let started = Instant::now();
    let Some(mut stream) =
        scan::TypedScanStream::open(cassie, session, collection, &fields, controls)?
    else {
        return Ok(None);
    };
    breakdown.scan += started.elapsed();
    let mut offset = usize::try_from(plan.offset.unwrap_or(0).max(0)).unwrap_or(usize::MAX);
    let mut remaining = plan.limit.map_or(usize::MAX, |limit| {
        usize::try_from(limit.max(0)).unwrap_or(usize::MAX)
    });
    let mut output = Vec::new();
    let mut scanned_rows = 0_usize;
    let mut output_memory = controls.reserve_query_memory(0)?;
    while remaining > 0 {
        let started = Instant::now();
        let source_bound = if plan.filter.is_some() && plan.limit.is_some() {
            1
        } else {
            remaining.saturating_add(offset)
        };
        let source_bound = source_bound.min(
            controls
                .max_result_rows
                .saturating_sub(output.len())
                .saturating_add(1),
        );
        let Some(mut batch) = stream.next_batch_bounded(source_bound)? else {
            break;
        };
        breakdown.scan += started.elapsed();
        scanned_rows = scanned_rows.saturating_add(batch.len());
        if let Some(predicate) = &plan.filter {
            let started = Instant::now();
            batch = batch.filter(controls, predicate, params, functions, session)?;
            breakdown.filter += started.elapsed();
        }
        let skip = offset.min(batch.len());
        offset -= skip;
        let count = (batch.len() - skip).min(remaining);
        if count == 0 {
            continue;
        }
        batch = batch.slice(controls, skip, count)?;
        remaining -= count;
        let started = Instant::now();
        let projected = batch.project(controls, &plan.projection, params, functions, session)?;
        breakdown.projection += started.elapsed();
        if output.len().saturating_add(projected.len()) > controls.max_result_rows {
            return Err(crate::app::CassieError::ResourceLimit(
                "query result row limit exceeded".to_owned(),
            )
            .into());
        }
        let started = Instant::now();
        output_memory.try_grow(mul(projected.len(), size_of::<BatchRow>())?)?;
        output.reserve_exact(projected.len());
        output.extend(output_rows(&projected, controls)?);
        breakdown.result_build += started.elapsed();
    }
    cassie
        .runtime
        .record_read_path_collection_scan(collection, fields.len(), scanned_rows);
    Ok(Some((output, breakdown)))
}

fn supports_plan(plan: &LogicalPlan) -> bool {
    !(plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
        || !plan.order.is_empty())
}

fn has_transport_budget(controls: &QueryExecutionControls) -> Result<bool, QueryError> {
    // Tight profiles retain scalar first-row admission and early stopping before
    // typed metadata is allocated or any storage source is opened.
    let carrier_floor = mul(
        crate::executor::batch::DEFAULT_BATCH_SIZE,
        size_of::<Value>(),
    )?;
    let available = controls
        .query_memory_budget_bytes
        .saturating_sub(controls.current_query_memory_bytes());
    Ok(available >= carrier_floor)
}

fn scan_fields(
    plan: &LogicalPlan,
    schema: &crate::catalog::CollectionSchema,
    controls: &QueryExecutionControls,
) -> Result<Option<(Vec<String>, crate::runtime::QueryMemoryReservation)>, QueryError> {
    let mut fields = Vec::new();
    let mut field_memory = controls.reserve_query_memory(0)?;
    for item in &plan.projection {
        match item {
            SelectItem::Column { name, .. } => push_field(&mut fields, &mut field_memory, name)?,
            SelectItem::Expr { expr, .. } => {
                if !collect_fields(expr, &mut fields, &mut field_memory)? {
                    return Ok(None);
                }
            }
            SelectItem::Wildcard => {
                if !schema.declares_id() {
                    push_field(&mut fields, &mut field_memory, "_id")?;
                }
                for field in &schema.fields {
                    field_memory.try_grow(mul(field.name.len(), 64)?)?;
                    let reference = crate::sql::ColumnIdentifierPath::stored_field_key(&field.name);
                    push_field(&mut fields, &mut field_memory, &reference)?;
                }
            }
            _ => return Ok(None),
        }
    }
    if let Some(expr) = &plan.filter {
        if !collect_fields(expr, &mut fields, &mut field_memory)? {
            return Ok(None);
        }
    }
    Ok(Some((fields, field_memory)))
}

fn push_field(
    fields: &mut Vec<String>,
    memory: &mut crate::runtime::QueryMemoryReservation,
    name: &str,
) -> Result<(), QueryError> {
    if !fields.iter().any(|field| field == name) {
        memory.try_grow(add(size_of::<String>(), mul(name.len(), 64)?)?)?;
        let name = crate::sql::ColumnIdentifierPath::reference_field_key(name);
        if fields.iter().any(|field| field == &name) {
            return Ok(());
        }
        fields.reserve_exact(1);
        fields.push(name);
    }
    Ok(())
}

fn collect_fields(
    expr: &Expr,
    fields: &mut Vec<String>,
    memory: &mut crate::runtime::QueryMemoryReservation,
) -> Result<bool, QueryError> {
    if matches!(expr, Expr::Exists(_) | Expr::Function(_)) {
        return Ok(false);
    }
    if let Expr::Column(name) = expr {
        push_field(fields, memory, name)?;
    }
    let mut accepted = Ok(true);
    expr.for_each_child(|child| {
        let previous = std::mem::replace(&mut accepted, Ok(false));
        accepted =
            previous.and_then(|accepted| Ok(accepted && collect_fields(child, fields, memory)?));
    });
    accepted
}

fn output_rows(
    batch: &TypedBatch,
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, QueryError> {
    let width = batch.schema().len();
    let mut bytes = mul(
        batch.len(),
        add(
            size_of::<BatchRow>(),
            mul(width, size_of::<(String, Value)>())?,
        )?,
    )?;
    bytes = add(
        bytes,
        add(
            size_of::<Vec<crate::types::DataType>>(),
            mul(width, size_of::<crate::types::DataType>())?,
        )?,
    )?;
    for (column, (name, data_type)) in batch.schema().iter().enumerate() {
        bytes = add(bytes, data_type_clone_bytes(data_type)?)?;
        bytes = add(bytes, mul(batch.len(), name.len())?)?;
        for lane in 0..batch.len() {
            bytes = add(
                bytes,
                crate::executor::typed_batch::scalar::cell_heap(batch.cell(column, lane)?)?,
            )?;
        }
    }
    let memory = Arc::new(controls.reserve_query_memory(bytes)?);
    let types = Arc::new(
        batch
            .schema()
            .iter()
            .map(|(_, data_type)| data_type.clone())
            .collect::<Vec<_>>(),
    );
    let mut rows = Vec::with_capacity(batch.len());
    for lane in 0..batch.len() {
        crate::executor::typed_batch::check_controls(controls)?;
        let mut values = Vec::with_capacity(width);
        for (column, (name, _)) in batch.schema().iter().enumerate() {
            values.push((name.clone(), batch.cell(column, lane)?.to_owned()));
        }
        let mut row =
            BatchRow::from_projected_values(values).with_query_memory(Some(Arc::clone(&memory)));
        row.set_data_types(Arc::clone(&types));
        rows.push(row);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_select_typed_transport_at_the_available_carrier_budget_boundary() {
        // Arrange
        let path =
            std::env::temp_dir().join(format!("cassie-typed-budget-{}", uuid::Uuid::new_v4()));
        let cassie = Cassie::new_with_data_dir(path.to_str().expect("path")).expect("cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE typed_budget (n BIGINT)",
            "INSERT INTO typed_budget VALUES (7)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect("setup");
        }
        let statement =
            crate::sql::parse_statement("SELECT n FROM typed_budget LIMIT 1").expect("statement");
        let plan =
            super::super::super::build_logical_plan_in_session(&cassie, Some(&session), &statement)
                .expect("plan");
        let floor = crate::executor::batch::DEFAULT_BATCH_SIZE * size_of::<Value>();
        let mut limits = cassie.runtime.limits();
        limits.query_memory_budget_bytes = floor + 1;
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let retained = controls.reserve_query_memory(2).expect("shared owner");

        // Act
        let fallback = try_execute(
            &cassie,
            Some(&session),
            &plan,
            &HashMap::new(),
            &[],
            &controls,
        )
        .expect("fallback");
        drop(retained);
        let rows = try_execute(
            &cassie,
            Some(&session),
            &plan,
            &HashMap::new(),
            &[],
            &controls,
        )
        .expect("execution")
        .expect("typed capability")
        .0;

        // Assert
        assert!(fallback.is_none());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].entries()[0].1, Value::Int64(7));
        drop(rows);
        assert_eq!(controls.current_query_memory_bytes(), 0);
        drop((session, cassie));
        std::fs::remove_dir_all(path).expect("cleanup");
    }

    #[test]
    fn should_discard_typed_output_when_a_later_storage_page_is_corrupt() {
        // Arrange
        let path =
            std::env::temp_dir().join(format!("cassie-typed-terminal-{}", uuid::Uuid::new_v4()));
        let cassie = Cassie::new_with_data_dir(path.to_str().expect("path")).expect("cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE typed_terminal (payload TEXT)",
                vec![],
            )
            .expect("table");
        let canary = "typed-terminal-corruption-canary";
        let documents = (0..=crate::executor::batch::DEFAULT_BATCH_SIZE)
            .map(|index| {
                let text = if index == crate::executor::batch::DEFAULT_BATCH_SIZE {
                    canary
                } else {
                    "valid"
                };
                (
                    Some(format!("doc-{index:05}")),
                    serde_json::json!({"payload": text}),
                )
            })
            .collect();
        cassie
            .midge
            .put_documents("typed_terminal", documents)
            .expect("seed");
        let (key, _) = cassie
            .midge
            .raw_scan_prefix_for_collection("typed_terminal", &[])
            .expect("raw entries")
            .into_iter()
            .find(|(_, value)| {
                value
                    .windows(canary.len())
                    .any(|bytes| bytes == canary.as_bytes())
            })
            .expect("last row");
        cassie
            .midge
            .raw_put(crate::midge::adapter::StorageFamily::Data, &key, &[u8::MAX])
            .expect("corrupt tail");
        let statement =
            crate::sql::parse_statement("SELECT payload FROM typed_terminal").expect("statement");
        let plan =
            super::super::super::build_logical_plan_in_session(&cassie, Some(&session), &statement)
                .expect("plan");
        let controls =
            QueryExecutionControls::from_limits(&cassie.runtime.limits(), Instant::now());

        // Act
        let result = try_execute(
            &cassie,
            Some(&session),
            &plan,
            &HashMap::new(),
            &[],
            &controls,
        );

        // Assert
        assert!(result.is_err());
        assert!(controls.peak_query_memory_bytes() > 0);
        assert_eq!(controls.current_query_memory_bytes(), 0);
        drop((session, cassie));
        std::fs::remove_dir_all(path).expect("cleanup");
    }

    #[test]
    fn should_keep_typed_columns_until_filtered_output_handoff() {
        // Arrange
        let path =
            std::env::temp_dir().join(format!("cassie-typed-pipeline-{}", uuid::Uuid::new_v4()));
        let cassie = Cassie::new_with_data_dir(path.to_str().expect("path")).expect("cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE typed (n BIGINT, flag BOOLEAN)",
            "INSERT INTO typed VALUES (9007199254740993, TRUE), (1, FALSE), (NULL, NULL)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect("setup");
        }
        let statement =
            crate::sql::parse_statement("SELECT n AS x, n AS y FROM typed WHERE flag OR n > 10")
                .expect("statement");
        let plan =
            super::super::super::build_logical_plan_in_session(&cassie, Some(&session), &statement)
                .expect("plan");
        let controls =
            QueryExecutionControls::from_limits(&cassie.runtime.limits(), Instant::now());

        // Act
        let (rows, _) = try_execute(
            &cassie,
            Some(&session),
            &plan,
            &HashMap::new(),
            &[],
            &controls,
        )
        .expect("execution")
        .expect("typed path");

        // Assert
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].entries(),
            &[
                ("x".to_owned(), Value::Int64(9_007_199_254_740_993)),
                ("y".to_owned(), Value::Int64(9_007_199_254_740_993))
            ]
        );
        assert_eq!(
            rows[0].data_types(),
            &[
                crate::types::DataType::BigInt,
                crate::types::DataType::BigInt
            ]
        );
        assert!(controls.current_query_memory_bytes() > 0);
        drop(rows);
        assert_eq!(controls.current_query_memory_bytes(), 0);
        drop((session, cassie));
        std::fs::remove_dir_all(path).expect("cleanup");
    }
}
