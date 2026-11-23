use super::command_output::empty as empty_command;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::app::CassieSession;

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
use crate::midge::adapter::documents::RollupSourceChangeReport;
use crate::midge::adapter::{DocumentWriteBatchOptions, DocumentWriteOp};

#[path = "rollups/incremental.rs"]
mod incremental;
#[path = "rollups/matching.rs"]
mod matching;
#[path = "rollups/output_order.rs"]
mod output_order;
use matching::{eligible_plan_shape, matching_rollup};
#[path = "rollup_expression.rs"]
mod rollup_expression;
#[path = "rollup_payload.rs"]
mod rollup_payload;

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
        return empty_command("CREATE ROLLUP", controls);
    }

    let meta = metadata_from_statement(cassie, statement, user_functions)?;
    ensure_current_rollup_format(&meta)?;
    cassie
        .midge
        .require_definition_version(meta.definition_semantics_version)
        .map_err(QueryError::Cassie)?;
    let source_generation = cassie
        .midge
        .collection_generation(&meta.source_collection)
        .map_err(QueryError::Cassie)?;
    let rows = build_rollup_rows(cassie, &meta, user_functions, controls)?;
    let rows = rollup_payload::serialize(rows, controls)?;
    ensure_source_generation(cassie, &meta, source_generation)?;

    let output = empty_command("CREATE ROLLUP", controls)?;
    cassie
        .midge
        .put_rollup(&meta)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_rollup(meta.clone());
    if let Err(error) = publish_rollup_rows(cassie, meta.clone(), rows, source_generation, controls)
    {
        if let Err(cleanup_error) = drop_rollup_data(cassie, &meta.name, false) {
            return Err(QueryError::General(format!(
                "rollup build failed: {error}; cleanup failed: {cleanup_error}"
            )));
        }
        return Err(error);
    }
    Ok(output)
}

pub(super) fn refresh_rollup(
    cassie: &Cassie,
    name: &str,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let output = empty_command("REFRESH ROLLUP", controls)?;
    refresh_rollup_data(cassie, name, user_functions, controls)?;
    Ok(output)
}

fn refresh_rollup_data(
    cassie: &Cassie,
    name: &str,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
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
    let rows = rollup_payload::serialize(rows, controls)?;
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
    publish_rollup_rows(cassie, current_meta, rows, source_generation, controls)?;
    Ok(())
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
    rows: rollup_payload::OwnedRollupRows,
    source_generation: u64,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    let output_collection = meta.output_collection.clone();
    // Gate only the output. Source writes must keep committing while a
    // publication is in flight; the generation checks mark it stale instead.
    cassie
        .midge
        .with_collection_gates(std::slice::from_ref(&output_collection), || {
            publish_rollup_rows_gated(cassie, meta, rows, source_generation, controls)
        })
}

fn publish_rollup_rows_gated(
    cassie: &Cassie,
    mut meta: RollupMeta,
    rows: rollup_payload::OwnedRollupRows,
    source_generation: u64,
    controls: &QueryExecutionControls,
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
    create_rollup_collection(cassie, &meta)?;
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
    meta.output_key_format = RollupMeta::CURRENT_OUTPUT_KEY_FORMAT;
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
    if generation_after_publication != source_generation {
        cassie
            .midge
            .put_rollup(&meta)
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie.catalog.register_rollup(meta.clone());
        return Err(QueryError::General(
            "source changed while rollup output was being published; retry refresh".to_string(),
        ));
    }
    replace_rollup_rows(cassie, &meta, rows, controls)?;
    cassie.catalog.register_rollup(meta.clone());
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
        && left.output_key_format == right.output_key_format
}

pub(super) fn drop_rollup(
    cassie: &Cassie,
    name: &str,
    if_exists: bool,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let output = empty_command("DROP ROLLUP", controls)?;
    drop_rollup_data(cassie, name, if_exists)?;
    Ok(output)
}

fn drop_rollup_data(cassie: &Cassie, name: &str, if_exists: bool) -> Result<(), QueryError> {
    let Some(meta) = cassie.catalog.get_rollup(name) else {
        if if_exists {
            return Ok(());
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
                    return Ok(());
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
            Ok(())
        })
}

pub(super) fn refresh_rollups_for_source(
    cassie: &Cassie,
    source: &str,
    report: &RollupSourceChangeReport,
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
                let incremental =
                    incremental::try_refresh_rollup(cassie, &rollup, report, controls)?;
                if !incremental {
                    refresh_rollup_data(cassie, &rollup.name, user_functions, controls)?;
                }
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
    session: Option<&CassieSession>,
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

    // A rollup holds committed rows only: a session with uncommitted changes to
    // the source must read the table, whichever write path staged them.
    if session.is_some_and(|session| session.has_collection_changes(source)) {
        cassie.runtime.record_rollup_fallback("session-changes");
        return Ok(None);
    }
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
        let order = output_order::rollup_output_order(&rollup, plan);
        let eval = sort::EvalInput {
            order: &order,
            projection: &[],
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
                cassie
                    .metadata_write_catalog
                    .register_rollup(current_rollup);
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
        bucket_expr: expr_key(&Expr::Function(Box::new(statement.bucket.clone()))),
        group_keys: statement.group_by.iter().map(group_expr_name).collect(),
        aggregates,
        filter_expr: statement.filter.as_ref().map(encode_filter).transpose()?,
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
        grouping_subtotal: false,
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
            function: *function,
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
    meta.filter_expr.as_deref().map(decode_filter).transpose()
}

/// The stored form of a rollup filter: canonical, fully parenthesized SQL that
/// parses back to the same tree. (`expr_key` is only a matching key, not SQL.)
pub(super) fn encode_filter(filter: &Expr) -> Result<String, QueryError> {
    rollup_expression::canonical_filter_expression(filter).map_err(QueryError::General)
}

fn decode_filter(stored: &str) -> Result<Expr, QueryError> {
    crate::sql::parser::parse_expression(stored).map_err(|error| {
        QueryError::General(format!(
            "rollup filter definition '{stored}' is unreadable ({error}); drop and recreate the rollup"
        ))
    })
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
        binding_context: None,
        user_functions,
        params: &[],
        parameter_type_oids: &[],
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
        let filtered = filter::filter_batches_with_controls(
            batches,
            filter_expr,
            &[],
            search_context.as_ref(),
            user_functions,
            None,
            controls,
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
            parameter_type_oids: &[],
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

fn replace_rollup_rows(
    cassie: &Cassie,
    meta: &RollupMeta,
    mut rows: rollup_payload::OwnedRollupRows,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    let operation_bytes = rows
        .len()
        .checked_mul(std::mem::size_of::<DocumentWriteOp>())
        .ok_or_else(|| QueryError::General("rollup write admission overflow".to_string()))?;
    let mut operation_memory = controls.reserve_query_memory(operation_bytes)?;
    let mut operations = Vec::new();
    operations
        .try_reserve_exact(rows.len())
        .map_err(|error| QueryError::General(error.to_string()))?;
    if operations.capacity() > rows.len() {
        return Err(QueryError::General(
            "rollup write allocation exceeded its admission".to_string(),
        ));
    }
    rows.publish_with(|_index, payload| {
        let id = stable_rollup_row_id(meta, &payload)?;
        let payload_bytes = rollup_payload::json_value_heap_size(&payload)?;
        let retained_bytes = id
            .capacity()
            .checked_add(payload_bytes)
            .ok_or_else(|| QueryError::General("rollup write admission overflow".to_string()))?;
        operation_memory.try_grow(retained_bytes)?;
        operations.push(DocumentWriteOp::Put { id, payload });
        super::check_timeout(controls)?;
        Ok(())
    })?;
    let (database, key, value) = cassie
        .midge
        .rollup_data_write(meta)
        .map_err(QueryError::Cassie)?;
    let mut options = DocumentWriteBatchOptions::sync(cassie.midge.write_options_sync());
    options.data_database = Some(database);
    options.additional_data_writes.push((key, Some(value)));
    cassie
        .midge
        .apply_document_write_batch_with_options(&meta.output_collection, operations, &options)
        .map_err(QueryError::Cassie)?;
    cassie
        .refresh_cardinality_stats(&meta.output_collection)
        .map_err(|error| QueryError::General(error.to_string()))?;
    Ok(())
}

fn stable_rollup_row_id(
    meta: &RollupMeta,
    payload: &serde_json::Value,
) -> Result<String, QueryError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut identity = Vec::with_capacity(meta.group_keys.len().saturating_add(1));
    identity.push(
        payload
            .get(&meta.bucket_expr)
            .ok_or_else(|| QueryError::General("rollup row is missing its bucket".to_string()))?,
    );
    for key in &meta.group_keys {
        identity.push(payload.get(key).ok_or_else(|| {
            QueryError::General(format!("rollup row is missing group key '{key}'"))
        })?);
    }
    let encoded =
        serde_json::to_vec(&identity).map_err(|error| QueryError::General(error.to_string()))?;
    let id_capacity = encoded
        .len()
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add("rollup-key-".len()))
        .ok_or_else(|| QueryError::General("rollup row key size overflow".to_string()))?;
    let mut id = String::with_capacity(id_capacity);
    id.push_str("rollup-key-");
    for byte in encoded {
        id.push(HEX[usize::from(byte >> 4)] as char);
        id.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    Ok(id)
}

fn stable_group_row_id(
    meta: &RollupMeta,
    bucket_seconds: i64,
    partition: &str,
) -> Result<String, QueryError> {
    let bucket = time::OffsetDateTime::from_unix_timestamp(bucket_seconds)
        .map_err(|error| QueryError::General(error.to_string()))?
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let mut payload = serde_json::Map::new();
    payload.insert(meta.bucket_expr.clone(), serde_json::Value::String(bucket));
    let Some(group_key) = meta.group_keys.first() else {
        return Err(QueryError::General(
            "incremental rollup has no group key".to_string(),
        ));
    };
    payload.insert(
        group_key.clone(),
        serde_json::Value::String(partition.to_owned()),
    );
    stable_rollup_row_id(meta, &serde_json::Value::Object(payload))
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
        Value::Array(value) => Ok(value.to_storage_json()),
        Value::Typed(value) => Ok(value.to_json()),
    }
}

fn now_ms() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| duration.as_millis().try_into().ok())
}
