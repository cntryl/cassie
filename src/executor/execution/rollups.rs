use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::catalog::{
    MaintenanceDebtMeta, RollupAggregateMeta, RollupDefinition, RollupMeta, RollupState,
};
use crate::executor::batch::{self, Batch, BatchRow};
use crate::sql::ast::{Expr, FunctionCall, QuerySource, SelectItem};
use crate::types::{DataType, FieldSchema, Schema, Value};

use super::source::{aggregate_signature, expr_key, group_expr_name};
use super::{
    aggregate_exec, check_timeout, filter, projection, scan, sort, Cassie, FunctionMeta,
    LogicalPlan, QueryError, QueryExecutionControls, QueryResult,
};
use crate::midge::adapter::check_rollup_maintenance_failure_point;

pub(super) fn create_rollup(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateRollupStatement,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    if statement.if_not_exists && cassie.catalog.get_rollup(&statement.name).is_some() {
        return Ok(empty_command("CREATE ROLLUP"));
    }

    let meta = metadata_from_statement(cassie, statement)?;
    create_rollup_collection(cassie, &meta)?;
    cassie
        .midge
        .put_rollup(&meta)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_rollup(meta.clone());
    refresh_rollup(cassie, &meta.name, user_functions, controls)?;
    Ok(empty_command("CREATE ROLLUP"))
}

pub(super) fn refresh_rollup(
    cassie: &Cassie,
    name: &str,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let mut meta = cassie
        .catalog
        .get_rollup(name)
        .ok_or_else(|| QueryError::General(format!("rollup '{name}' does not exist")))?;
    meta.state = RollupState::Building;
    cassie.catalog.register_rollup(meta.clone());
    cassie
        .midge
        .put_rollup(&meta)
        .map_err(|error| QueryError::General(error.to_string()))?;

    let rows = build_rollup_rows(cassie, &meta, user_functions, controls)?;
    replace_rollup_rows(cassie, &meta, rows)?;
    let source_generation = cassie
        .midge
        .collection_generation(&meta.source_collection)
        .map_err(QueryError::Cassie)?;
    meta.state = RollupState::Ready;
    meta.refresh_cursor.last_refresh_ms = now_ms();
    meta.refresh_cursor.source_generation = source_generation;
    meta.refresh_cursor.source_epoch = cassie.runtime.data_epoch();
    meta.refresh_cursor.source_row_count = cassie
        .catalog
        .get_cardinality_stats(&meta.source_collection)
        .map_or(0, |stats| stats.row_count);
    meta.refresh_cursor.lag_rows = 0;
    cassie
        .midge
        .put_rollup(&meta)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_rollup(meta.clone());
    cassie.runtime.record_rollup_refresh(meta.name);
    Ok(empty_command("REFRESH ROLLUP"))
}

pub(super) fn drop_rollup(
    cassie: &Cassie,
    name: &str,
    if_exists: bool,
) -> Result<QueryResult, QueryError> {
    let Some(meta) = cassie.catalog.get_rollup(name) else {
        if if_exists {
            return Ok(empty_command("DROP ROLLUP"));
        }
        return Err(QueryError::General(format!(
            "rollup '{name}' does not exist"
        )));
    };
    let _ = cassie.midge.drop_collection(&meta.output_collection);
    cassie
        .catalog
        .unregister_collection(&meta.output_collection);
    cassie
        .midge
        .delete_rollup(&meta.name)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.unregister_rollup(&meta.name);
    Ok(empty_command("DROP ROLLUP"))
}

pub(super) fn refresh_rollups_for_source(
    cassie: &Cassie,
    source: &str,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    let generation = cassie
        .midge
        .collection_generation(source)
        .map_err(QueryError::Cassie)?;
    let refresh = check_rollup_maintenance_failure_point()
        .map_err(QueryError::Cassie)
        .and_then(|()| {
            for rollup in cassie.catalog.list_rollups_for_source(source) {
                refresh_rollup(cassie, &rollup.name, user_functions, controls)?;
            }
            Ok(())
        });
    match refresh {
        Ok(()) => {
            let _ = cassie
                .midge
                .clear_rollup_maintenance_debt(source, generation);
            let _ = sync_rollup_debt_catalog(cassie, source);
        }
        Err(error) => {
            let storage_error = crate::app::CassieError::Execution(error.to_string());
            let _ =
                cassie
                    .midge
                    .record_rollup_maintenance_failure(source, generation, &storage_error);
            let _ = sync_rollup_debt_catalog(cassie, source);
        }
    }
    Ok(())
}

pub(super) fn try_execute_rollup_query(
    cassie: &Cassie,
    plan: &LogicalPlan,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    if !eligible_plan_shape(plan) {
        return Ok(None);
    }
    let QuerySource::Collection(source) = &plan.source else {
        return Ok(None);
    };

    let Some(rollup) = matching_rollup(cassie, source, plan) else {
        cassie.runtime.record_rollup_fallback("no-match");
        return Ok(None);
    };
    if cassie
        .midge
        .has_rollup_maintenance_debt(source)
        .map_err(QueryError::Cassie)?
    {
        cassie.runtime.record_rollup_fallback("maintenance_pending");
        return Ok(None);
    }
    let source_generation = cassie
        .midge
        .collection_generation(source)
        .map_err(QueryError::Cassie)?;
    if !rollup.is_fresh(source_generation) {
        cassie.runtime.record_rollup_fallback("stale");
        return Ok(None);
    }

    let mut batches = scan::scan(cassie, None, &rollup.output_collection)?;
    if !plan.order.is_empty() {
        batches = sort::sort_batches(
            batches,
            &plan.order,
            &plan.projection,
            params,
            None,
            user_functions,
            None,
        );
    }
    batches = projection::project_batches(
        batches,
        &plan.projection,
        params,
        None,
        user_functions,
        None,
    )?;
    let rows = batch::flatten_batches(batches);
    let rows = super::source::slice_rows(rows, plan.offset, plan.limit);
    cassie.runtime.record_rollup_rewrite(rollup.name);
    check_timeout(controls)?;
    Ok(Some(rows))
}

pub(super) fn mark_source_rollups_stale(cassie: &Cassie, source: &str) -> Result<(), QueryError> {
    for mut rollup in cassie.catalog.list_rollups_for_source(source) {
        rollup.state = RollupState::Stale;
        rollup.refresh_cursor.lag_rows = rollup.refresh_cursor.lag_rows.saturating_add(1);
        cassie
            .midge
            .put_rollup(&rollup)
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie.catalog.register_rollup(rollup);
    }
    Ok(())
}

pub(super) fn rewrite_name_for_plan(cassie: &Cassie, plan: &LogicalPlan) -> Option<String> {
    if !eligible_plan_shape(plan) {
        return None;
    }
    let QuerySource::Collection(source) = &plan.source else {
        return None;
    };
    if cassie
        .midge
        .has_rollup_maintenance_debt(source)
        .ok()
        .is_none_or(|pending| pending)
    {
        return None;
    }
    let source_generation = cassie
        .midge
        .collection_generation(source)
        .ok()?;
    matching_rollup(cassie, source, plan)
        .filter(|rollup| rollup.is_fresh(source_generation))
        .map(|rollup| rollup.name)
}

pub(super) fn sync_rollup_debt_catalog(cassie: &Cassie, source: &str) -> Result<(), QueryError> {
    let Some(debt) = cassie
        .midge
        .maintenance_debt_for(source, "rollup")
        .map_err(QueryError::Cassie)?
    else {
        cassie.catalog.unregister_maintenance_debt(source, "rollup");
        return Ok(());
    };
    cassie
        .catalog
        .register_maintenance_debt(MaintenanceDebtMeta::new(
            debt.collection,
            debt.artifact,
            debt.target_generation,
            debt.retry_count,
            debt.last_error,
        ));
    Ok(())
}

fn metadata_from_statement(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateRollupStatement,
) -> Result<RollupMeta, QueryError> {
    let Expr::StringLiteral(width) = &statement.bucket.args[0] else {
        return Err(QueryError::General(
            "rollup time_bucket width must be a string literal".to_string(),
        ));
    };
    let Expr::Column(timestamp_field) = &statement.bucket.args[1] else {
        return Err(QueryError::General(
            "rollup time_bucket timestamp must be a column".to_string(),
        ));
    };
    let origin = statement.bucket.args.get(2).map(expr_key);
    let aggregates = statement
        .aggregates
        .iter()
        .map(|item| aggregate_meta(cassie, &statement.source, item))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RollupMeta::new(RollupDefinition {
        name: statement.name.clone(),
        source_collection: statement.source.clone(),
        timestamp_field: timestamp_field.clone(),
        bucket_width: width.clone(),
        origin,
        bucket_expr: expr_key(&Expr::Function(statement.bucket.clone())),
        group_keys: statement.group_by.iter().map(group_expr_name).collect(),
        aggregates,
        filter_expr: statement.filter.as_ref().map(expr_key),
    }))
}

fn aggregate_meta(
    cassie: &Cassie,
    source: &str,
    item: &SelectItem,
) -> Result<RollupAggregateMeta, QueryError> {
    let SelectItem::Function { function, alias } = item else {
        return Err(QueryError::General(
            "rollup aggregate metadata requires a function".to_string(),
        ));
    };
    let alias = alias
        .clone()
        .unwrap_or_else(|| aggregate_signature(function));
    Ok(RollupAggregateMeta {
        alias,
        function: function.name.to_ascii_lowercase(),
        expression: aggregate_signature(function),
        data_type: aggregate_data_type(cassie, source, function),
    })
}

fn aggregate_data_type(cassie: &Cassie, source: &str, function: &FunctionCall) -> DataType {
    match function.name.to_ascii_lowercase().as_str() {
        "count" => DataType::BigInt,
        "sum" => function
            .args
            .first()
            .and_then(|expr| match expr {
                Expr::Column(name) => cassie.catalog.field_type(source, name),
                _ => None,
            })
            .unwrap_or(DataType::Float),
        "avg" => DataType::Float,
        "min" | "max" => function
            .args
            .first()
            .and_then(|expr| match expr {
                Expr::Column(name) => cassie.catalog.field_type(source, name),
                _ => None,
            })
            .unwrap_or(DataType::Text),
        _ => DataType::Text,
    }
}

fn create_rollup_collection(cassie: &Cassie, meta: &RollupMeta) -> Result<(), QueryError> {
    let schema = rollup_schema(cassie, meta);
    let _ = cassie.midge.drop_collection(&meta.output_collection);
    cassie
        .catalog
        .unregister_collection(&meta.output_collection);
    cassie
        .midge
        .create_collection(&meta.output_collection, schema.clone())
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_collection(
        &meta.output_collection,
        schema
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.data_type.clone()))
            .collect(),
    );
    Ok(())
}

fn rollup_schema(cassie: &Cassie, meta: &RollupMeta) -> Schema {
    let mut fields = vec![FieldSchema {
        name: meta.bucket_expr.clone(),
        data_type: DataType::Text,
        nullable: true,
    }];
    for key in &meta.group_keys {
        fields.push(FieldSchema {
            name: key.clone(),
            data_type: cassie
                .catalog
                .field_type(&meta.source_collection, key)
                .unwrap_or(DataType::Text),
            nullable: true,
        });
    }
    fields.extend(meta.aggregates.iter().map(|aggregate| FieldSchema {
        name: aggregate.alias.clone(),
        data_type: aggregate.data_type.clone(),
        nullable: true,
    }));
    Schema { fields }
}

fn build_rollup_rows(
    cassie: &Cassie,
    meta: &RollupMeta,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, QueryError> {
    let plan = build_rollup_refresh_plan(meta)?;
    let (batches, search_context) =
        read_rollup_source_batches(cassie, &plan, user_functions, controls)?;
    let batches = materialize_rollup_batches(
        cassie,
        batches,
        &plan,
        search_context.as_ref(),
        user_functions,
        controls,
    )?;
    Ok(batch::flatten_batches(batches))
}

fn build_rollup_refresh_plan(meta: &RollupMeta) -> Result<LogicalPlan, QueryError> {
    Ok(LogicalPlan {
        command: None,
        source: QuerySource::Collection(meta.source_collection.clone()),
        collection: meta.source_collection.clone(),
        ctes: Vec::new(),
        distinct: false,
        distinct_on: Vec::new(),
        projection: rollup_projection(meta)?,
        filter: rollup_filter(meta)?,
        group_by: rollup_group_by(meta)?,
        having: None,
        order: Vec::new(),
        limit: None,
        offset: None,
        set: None,
    })
}

fn rollup_projection(meta: &RollupMeta) -> Result<Vec<SelectItem>, QueryError> {
    let mut projection = Vec::new();
    projection.push(SelectItem::Expr {
        expr: crate::sql::parser::parse_expression(&meta.bucket_expr)
            .map_err(|error| QueryError::General(error.to_string()))?,
        alias: Some(meta.bucket_expr.clone()),
    });
    projection.extend(meta.group_keys.iter().map(|name| SelectItem::Column {
        name: name.clone(),
        alias: None,
    }));
    for aggregate in &meta.aggregates {
        let Expr::Function(function) = crate::sql::parser::parse_expression(&aggregate.expression)
            .map_err(|error| QueryError::General(error.to_string()))?
        else {
            return Err(QueryError::General("invalid rollup aggregate".to_string()));
        };
        projection.push(SelectItem::Function {
            function,
            alias: Some(aggregate.alias.clone()),
        });
    }
    Ok(projection)
}

fn rollup_group_by(meta: &RollupMeta) -> Result<Vec<Expr>, QueryError> {
    Ok(std::iter::once(
        crate::sql::parser::parse_expression(&meta.bucket_expr)
            .map_err(|error| QueryError::General(error.to_string()))?,
    )
    .chain(meta.group_keys.iter().map(|key| Expr::Column(key.clone())))
    .collect::<Vec<_>>())
}

fn rollup_filter(meta: &RollupMeta) -> Result<Option<Expr>, QueryError> {
    meta.filter_expr
        .as_ref()
        .map(|raw| crate::sql::parser::parse_expression(raw))
        .transpose()
        .map_err(|error| QueryError::General(error.to_string()))
}

fn read_rollup_source_batches(
    cassie: &Cassie,
    plan: &LogicalPlan,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<(Vec<Batch>, Option<filter::SearchContext>), QueryError> {
    let env = super::source::SourceExecutionEnv {
        cassie,
        session: None,
        user_functions,
        params: &[],
        controls,
    };
    let (batches, text_fields) = super::source::execute_query_source(
        &env,
        &plan.source,
        &mut HashMap::new(),
        false,
        None,
        None,
    )?;
    let search_context = if text_fields.is_empty() {
        None
    } else {
        Some(filter::SearchContext::from_rows(
            batches.iter().flat_map(|batch| batch.iter()),
            &text_fields,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        ))
    };
    if let Some(filter_expr) = &plan.filter {
        let filtered = filter::filter_batches(
            batches,
            filter_expr,
            &[],
            search_context.as_ref(),
            user_functions,
            None,
        )?;
        return Ok((filtered, search_context));
    }
    Ok((batches, search_context))
}

fn materialize_rollup_batches(
    cassie: &Cassie,
    batches: Vec<Batch>,
    plan: &LogicalPlan,
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, QueryError> {
    let batches = aggregate_exec::aggregate_query_batches(
        cassie,
        batches,
        &aggregate_exec::AggregateExecutionContext {
            plan,
            params: &[],
            search_context,
            user_functions,
            session: None,
            controls,
        },
    )?;
    projection::project_batches(
        batches,
        &plan.projection,
        &[],
        search_context,
        user_functions,
        None,
    )
}

fn replace_rollup_rows(
    cassie: &Cassie,
    meta: &RollupMeta,
    rows: Vec<BatchRow>,
) -> Result<(), QueryError> {
    create_rollup_collection(cassie, meta)?;
    for (index, row) in rows.into_iter().enumerate() {
        let payload = row
            .into_entries()
            .into_iter()
            .map(|(name, value)| (name, value_to_json(value)))
            .collect::<serde_json::Map<_, _>>();
        cassie
            .midge
            .put_document(
                &meta.output_collection,
                Some(format!("rollup-row-{index:020}")),
                serde_json::Value::Object(payload),
            )
            .map_err(|error| QueryError::General(error.to_string()))?;
    }
    cassie
        .refresh_cardinality_stats(&meta.output_collection)
        .map_err(|error| QueryError::General(error.to_string()))?;
    Ok(())
}

fn matching_rollup(cassie: &Cassie, source: &str, plan: &LogicalPlan) -> Option<RollupMeta> {
    cassie
        .catalog
        .list_rollups_for_source(source)
        .into_iter()
        .find(|rollup| rollup_matches_plan(rollup, plan))
}

fn rollup_matches_plan(rollup: &RollupMeta, plan: &LogicalPlan) -> bool {
    let expected_groups = std::iter::once(rollup.bucket_expr.clone())
        .chain(rollup.group_keys.iter().cloned())
        .collect::<Vec<_>>();
    let actual_groups = plan.group_by.iter().map(expr_key).collect::<Vec<_>>();
    if actual_groups != expected_groups {
        return false;
    }
    if plan.filter.as_ref().map(expr_key) != rollup.filter_expr {
        return false;
    }
    let expected_aggregates = rollup
        .aggregates
        .iter()
        .map(|aggregate| aggregate.expression.clone())
        .collect::<Vec<_>>();
    let actual_aggregates = plan_aggregate_signatures(plan);
    actual_aggregates == expected_aggregates
}

fn plan_aggregate_signatures(plan: &LogicalPlan) -> Vec<String> {
    plan.projection
        .iter()
        .filter_map(|item| match item {
            SelectItem::Function { function, .. }
                if crate::sql::functions::is_aggregate_function(&function.name) =>
            {
                Some(aggregate_signature(function))
            }
            _ => None,
        })
        .collect()
}

fn eligible_plan_shape(plan: &LogicalPlan) -> bool {
    matches!(plan.source, QuerySource::Collection(_))
        && plan.command.is_none()
        && !plan.group_by.is_empty()
        && plan.having.is_none()
        && plan.set.is_none()
        && plan.ctes.is_empty()
        && !plan.distinct
        && plan.distinct_on.is_empty()
}

fn value_to_json(value: Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(value) => serde_json::Value::Bool(value),
        Value::Int64(value) => serde_json::Value::Number(value.into()),
        Value::Float64(value) => serde_json::Number::from_f64(value)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        Value::String(value) => serde_json::Value::String(value),
        Value::Vector(value) => serde_json::json!(value.values),
        Value::Json(value) => value,
    }
}

fn empty_command(command: &str) -> QueryResult {
    QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
        command: command.to_string(),
    }
}

fn now_ms() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| duration.as_millis().try_into().ok())
}
