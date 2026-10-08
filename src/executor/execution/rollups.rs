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
    aggregate_exec, check_timeout, filter, projection, reserve_projection_output_before_building,
    scan, sort, Cassie, FunctionMeta, LogicalPlan, QueryError, QueryExecutionControls, QueryResult,
};
use crate::midge::adapter::check_rollup_maintenance_failure_point;

#[path = "rollup_expression.rs"]
mod rollup_expression;

pub(super) fn create_rollup(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateRollupStatement,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let output_collection = crate::catalog::output_collection_name(&statement.name);
    cassie
        .midge
        .with_collection_gates(std::slice::from_ref(&output_collection), || {
            create_rollup_gated(cassie, statement, user_functions, controls)
        })
}

fn create_rollup_gated(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateRollupStatement,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    if statement.if_not_exists && cassie.catalog.get_rollup(&statement.name).is_some() {
        return Ok(empty_command("CREATE ROLLUP"));
    }

    let meta = metadata_from_statement(cassie, statement, user_functions)?;
    ensure_current_rollup_format(&meta)?;
    let source_generation = cassie
        .midge
        .collection_generation(&meta.source_collection)
        .map_err(QueryError::Cassie)?;
    let rows = build_rollup_rows(cassie, &meta, user_functions, controls)?;
    let rows = serialize_rollup_rows(rows)?;
    ensure_source_generation(cassie, &meta, source_generation)?;

    cassie
        .midge
        .put_rollup(&meta)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_rollup(meta.clone());
    if let Err(error) = publish_rollup_rows(cassie, meta.clone(), rows, source_generation) {
        if let Err(cleanup_error) = drop_rollup(cassie, &meta.name, false) {
            return Err(QueryError::General(format!(
                "rollup build failed: {error}; cleanup failed: {cleanup_error}"
            )));
        }
        return Err(error);
    }
    Ok(empty_command("CREATE ROLLUP"))
}

pub(super) fn refresh_rollup(
    cassie: &Cassie,
    name: &str,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let meta = cassie
        .catalog
        .get_rollup(name)
        .ok_or_else(|| QueryError::General(format!("rollup '{name}' does not exist")))?;
    ensure_current_rollup_format(&meta)?;
    crate::sql::definition_guard::rollup(&meta).map_err(QueryError::Cassie)?;
    let source_generation = cassie
        .midge
        .collection_generation(&meta.source_collection)
        .map_err(QueryError::Cassie)?;
    let rows = build_rollup_rows(cassie, &meta, user_functions, controls)?;
    let rows = serialize_rollup_rows(rows)?;
    ensure_source_generation(cassie, &meta, source_generation)?;
    let current_meta = cassie
        .catalog
        .get_rollup(name)
        .ok_or_else(|| QueryError::General(format!("rollup '{name}' does not exist")))?;
    ensure_current_rollup_format(&current_meta)?;
    if !same_rollup_definition(&meta, &current_meta) {
        return Err(QueryError::General(format!(
            "rollup '{name}' definition changed while refresh was being prepared; retry refresh"
        )));
    }
    publish_rollup_rows(cassie, current_meta, rows, source_generation)?;
    Ok(empty_command("REFRESH ROLLUP"))
}

fn ensure_source_generation(
    cassie: &Cassie,
    meta: &RollupMeta,
    expected_generation: u64,
) -> Result<(), QueryError> {
    let current_generation = cassie
        .midge
        .collection_generation(&meta.source_collection)
        .map_err(QueryError::Cassie)?;
    if current_generation != expected_generation {
        return Err(QueryError::General(
            "source changed while rollup output was being prepared; retry refresh".to_string(),
        ));
    }
    Ok(())
}

fn publish_rollup_rows(
    cassie: &Cassie,
    meta: RollupMeta,
    rows: Vec<serde_json::Value>,
    source_generation: u64,
) -> Result<(), QueryError> {
    let output_collection = meta.output_collection.clone();
    cassie
        .midge
        .with_collection_gates(std::slice::from_ref(&output_collection), || {
            publish_rollup_rows_gated(cassie, meta, rows, source_generation)
        })
}

fn publish_rollup_rows_gated(
    cassie: &Cassie,
    mut meta: RollupMeta,
    rows: Vec<serde_json::Value>,
    source_generation: u64,
) -> Result<(), QueryError> {
    ensure_current_rollup_format(&meta)?;
    let current_meta = cassie
        .catalog
        .get_rollup(&meta.name)
        .ok_or_else(|| QueryError::General(format!("rollup '{}' does not exist", meta.name)))?;
    if !same_rollup_definition(&meta, &current_meta) {
        return Err(QueryError::General(format!(
            "rollup '{}' definition changed before publication; retry refresh",
            meta.name
        )));
    }
    ensure_source_generation(cassie, &meta, source_generation)?;
    meta.state = RollupState::Building;
    cassie
        .midge
        .put_rollup(&meta)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_rollup(meta.clone());

    crate::executor::pause_before_rollup_output_replace(&meta.name, source_generation);
    replace_rollup_rows(cassie, &meta, rows)?;
    crate::executor::pause_before_rollup_ready_metadata(&meta.name, source_generation);
    let generation_after_publication = cassie
        .midge
        .collection_generation(&meta.source_collection)
        .map_err(QueryError::Cassie)?;
    meta.state = if generation_after_publication == source_generation {
        RollupState::Ready
    } else {
        RollupState::Stale
    };
    meta.refresh_cursor.last_refresh_ms = now_ms();
    meta.refresh_cursor.source_generation = source_generation;
    meta.refresh_cursor.source_epoch = cassie.runtime.data_epoch();
    meta.refresh_cursor.source_row_count = cassie
        .catalog
        .get_cardinality_stats(&meta.source_collection)
        .map_or(0, |stats| stats.row_count);
    meta.refresh_cursor.lag_rows = if generation_after_publication == source_generation {
        0
    } else {
        meta.refresh_cursor.lag_rows.saturating_add(1)
    };
    cassie
        .midge
        .put_rollup(&meta)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_rollup(meta.clone());
    if generation_after_publication != source_generation {
        return Err(QueryError::General(
            "source changed while rollup output was being published; retry refresh".to_string(),
        ));
    }
    cassie.runtime.record_rollup_refresh(meta.name);
    Ok(())
}

fn ensure_current_rollup_format(meta: &RollupMeta) -> Result<(), QueryError> {
    if meta.version != RollupMeta::CURRENT_VERSION {
        return Err(QueryError::General(format!(
            "rollup '{}' uses unsupported definition version {}; drop and recreate it",
            meta.name, meta.version
        )));
    }
    Ok(())
}

fn same_rollup_definition(left: &RollupMeta, right: &RollupMeta) -> bool {
    left.name == right.name
        && left.source_collection == right.source_collection
        && left.output_collection == right.output_collection
        && left.timestamp_field == right.timestamp_field
        && left.bucket_width == right.bucket_width
        && left.origin == right.origin
        && left.bucket_expr == right.bucket_expr
        && left.group_keys == right.group_keys
        && left.aggregates == right.aggregates
        && left.filter_expr == right.filter_expr
        && left.version == right.version
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
    cassie
        .midge
        .with_collection_gates(std::slice::from_ref(&meta.output_collection), || {
            let Some(current_meta) = cassie.catalog.get_rollup(name) else {
                if if_exists {
                    return Ok(empty_command("DROP ROLLUP"));
                }
                return Err(QueryError::General(format!(
                    "rollup '{name}' does not exist"
                )));
            };
            let _ = cassie
                .midge
                .drop_collection(&current_meta.output_collection);
            let _ = cassie
                .catalog
                .unregister_collection(&current_meta.output_collection);
            cassie
                .midge
                .delete_rollup(&current_meta.name)
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.unregister_rollup(&current_meta.name);
            Ok(empty_command("DROP ROLLUP"))
        })
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

    let mut batches = scan::scan(cassie, None, &rollup.output_collection, controls)?;
    if !plan.order.is_empty() {
        let eval = sort::EvalInput {
            order: &plan.order,
            projection: &plan.projection,
            params,
            user_functions,
            search_context: None,
            session: None,
        };
        batches = sort::sort_batches_with_controls(batches, &eval, controls)?;
    }
    let _projected_output_memory =
        reserve_projection_output_before_building(controls, &batches, &plan.projection)?;
    batches = projection::project_batches(
        batches,
        &plan.projection,
        params,
        None,
        user_functions,
        None,
    )?;
    let rows = batch::flatten_batches(batches);
    let rows = super::source::slice_rows(rows, plan.offset_value(), plan.limit_value());
    cassie.runtime.record_rollup_rewrite(rollup.name);
    check_timeout(controls)?;
    Ok(Some(rows))
}

pub(super) fn mark_source_rollups_stale(cassie: &Cassie, source: &str) -> Result<(), QueryError> {
    for rollup in cassie.catalog.list_rollups_for_source(source) {
        cassie.midge.with_collection_gates(
            std::slice::from_ref(&rollup.output_collection),
            || -> Result<(), QueryError> {
                let Some(mut current_rollup) = cassie.catalog.get_rollup(&rollup.name) else {
                    return Ok(());
                };
                current_rollup.state = RollupState::Stale;
                current_rollup.refresh_cursor.lag_rows =
                    current_rollup.refresh_cursor.lag_rows.saturating_add(1);
                cassie
                    .midge
                    .put_rollup(&current_rollup)
                    .map_err(|error| QueryError::General(error.to_string()))?;
                cassie.catalog.register_rollup(current_rollup);
                Ok(())
            },
        )?;
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
    let source_generation = cassie.midge.collection_generation(source).ok()?;
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
    user_functions: &HashMap<String, FunctionMeta>,
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
        .map(|item| aggregate_meta(cassie, &statement.source, item, user_functions))
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
    user_functions: &HashMap<String, FunctionMeta>,
) -> Result<RollupAggregateMeta, QueryError> {
    let SelectItem::Function { function, alias } = item else {
        return Err(QueryError::General(
            "rollup aggregate metadata requires a function".to_string(),
        ));
    };
    let alias = alias
        .clone()
        .unwrap_or_else(|| aggregate_signature(function));
    let expression =
        rollup_expression::canonical_aggregate_expression(function).map_err(QueryError::General)?;
    Ok(RollupAggregateMeta {
        alias,
        function: function.name.to_ascii_lowercase(),
        expression,
        data_type: aggregate_data_type(cassie, source, function, user_functions),
    })
}

fn aggregate_data_type(
    cassie: &Cassie,
    source: &str,
    function: &FunctionCall,
    user_functions: &HashMap<String, FunctionMeta>,
) -> DataType {
    let source_schema = cassie
        .midge
        .collection_schema(source)
        .unwrap_or_else(|| Schema { fields: Vec::new() });
    let argument_type = function.args.first().and_then(|expression| {
        crate::sql::binder::infer_expr_type(expression, &source_schema, user_functions, &[])
    });
    match function.name.to_ascii_lowercase().as_str() {
        "count" => DataType::BigInt,
        // SUM widens every integer input to int8, exactly as the query path
        // types it (`FunctionReturnType::SumArgument`), so a bucket total
        // that exceeds the source column's range still fits.
        "sum" => match argument_type {
            Some(DataType::SmallInt | DataType::Int | DataType::BigInt) => DataType::BigInt,
            Some(_) | None => DataType::Float,
        },
        "avg" => DataType::Float,
        "min" | "max" => argument_type.unwrap_or(DataType::Text),
        _ => DataType::Text,
    }
}

fn create_rollup_collection(cassie: &Cassie, meta: &RollupMeta) -> Result<(), QueryError> {
    let schema = rollup_schema(cassie, meta);
    let _ = cassie.midge.drop_collection(&meta.output_collection);
    let _ = cassie
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
    ensure_current_rollup_format(meta)?;
    Ok(LogicalPlan {
        command: None,
        source: QuerySource::Collection(
            crate::sql::IdentifierPath::parse(&meta.source_collection)
                .map_err(QueryError::General)?,
        ),
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
        &mut super::CteContext::new(),
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
            #[cfg(test)]
            after_partition_row: None,
        },
    )?;
    let _projected_output_memory =
        reserve_projection_output_before_building(controls, &batches, &plan.projection)?;
    projection::project_batches(
        batches,
        &plan.projection,
        &[],
        search_context,
        user_functions,
        None,
    )
}

fn serialize_rollup_rows(rows: Vec<BatchRow>) -> Result<Vec<serde_json::Value>, QueryError> {
    rows.into_iter()
        .map(|row| {
            let _operator_parent = row.operator_memory();
            let payload = row
                .into_entries()
                .into_iter()
                .map(|(name, value)| Ok((name, value_to_json(value)?)))
                .collect::<Result<serde_json::Map<_, _>, QueryError>>()?;
            Ok(serde_json::Value::Object(payload))
        })
        .collect()
}

fn replace_rollup_rows(
    cassie: &Cassie,
    meta: &RollupMeta,
    rows: Vec<serde_json::Value>,
) -> Result<(), QueryError> {
    create_rollup_collection(cassie, meta)?;
    for (index, payload) in rows.into_iter().enumerate() {
        cassie
            .midge
            .put_document(
                &meta.output_collection,
                Some(format!("rollup-row-{index:020}")),
                payload,
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
    if rollup.version != RollupMeta::CURRENT_VERSION {
        return false;
    }
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
    let Some(expected_aggregates) = rollup
        .aggregates
        .iter()
        .map(stored_aggregate_signature)
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    let actual_aggregates = plan_aggregate_signatures(plan);
    actual_aggregates == expected_aggregates
}

fn stored_aggregate_signature(aggregate: &RollupAggregateMeta) -> Option<String> {
    let Expr::Function(function) =
        crate::sql::parser::parse_expression(&aggregate.expression).ok()?
    else {
        return None;
    };
    if !function.name.eq_ignore_ascii_case(&aggregate.function) {
        return None;
    }
    Some(aggregate_signature(&function))
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
        && !plan
            .projection
            .iter()
            .any(|item| matches!(item, SelectItem::WindowFunction { .. }))
}

fn value_to_json(value: Value) -> Result<serde_json::Value, QueryError> {
    match value {
        Value::Null => Ok(serde_json::Value::Null),
        Value::Bool(value) => Ok(serde_json::Value::Bool(value)),
        Value::Int64(value) => Ok(serde_json::Value::Number(value.into())),
        Value::Float64(value) => serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| {
                QueryError::General(
                    "non-finite FLOAT values cannot be stored in rollup outputs".to_string(),
                )
            }),
        Value::String(value) => Ok(serde_json::Value::String(value)),
        Value::Vector(value) => Ok(serde_json::json!(value.values)),
        Value::Json(value) => Ok(value),
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
