use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

pub(super) fn reject_write(cassie: &Cassie, relation: &str) -> Result<(), QueryError> {
    if cassie.catalog.is_materialized_projection(relation)
        || cassie
            .catalog
            .materialized_projection_for_output(relation)
            .is_some()
    {
        return Err(QueryError::General(format!(
            "materialized projection '{relation}' is read-only"
        )));
    }
    Ok(())
}

pub(super) fn mark_source_projections_stale(
    cassie: &Cassie,
    source: &str,
) -> Result<(), QueryError> {
    for mut projection in cassie.catalog.list_projection_metadata() {
        let Some(materialized) = projection.materialized.as_mut() else {
            continue;
        };
        if !materialized
            .source_collections
            .iter()
            .any(|candidate| candidate == source)
        {
            continue;
        }
        materialized.state = catalog::MaterializedProjectionState::Stale;
        projection.freshness = catalog::ProjectionFreshness::Stale;
        projection.lag = projection.lag.saturating_add(1);
        cassie
            .runtime
            .record_projection_stale_mark(projection.collection.clone());
        persist_projection_metadata(cassie, projection)?;
    }
    Ok(())
}

pub(super) fn create_materialized_projection(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateMaterializedProjectionStatement,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    if statement.if_not_exists && cassie.catalog.relation_exists(&statement.name) {
        return Ok(empty_command("CREATE MATERIALIZED PROJECTION"));
    }
    if cassie.catalog.relation_exists(&statement.name)
        || virtual_views::schema(&statement.name).is_some()
    {
        return Err(QueryError::General(format!(
            "relation '{}' already exists",
            statement.name
        )));
    }

    let build = plan_projection_query(cassie, &statement.query, user_functions)?;
    reject_unsupported_sources(cassie, &build.source_collections)?;
    reject_nondeterministic_select(&build.select, user_functions)?;

    let metadata = catalog::ProjectionMeta::materialized(
        statement.name.clone(),
        statement.query.clone(),
        build.source_collections,
        build.schema.clone(),
        cassie.catalog.version(),
        stable_projection_fingerprint(&statement.query),
        now_ms(),
    );
    cassie
        .midge
        .put_projection_metadata(metadata.clone())
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_projection_metadata(metadata);
    refresh_materialized_projection(cassie, &statement.name, user_functions, controls)?;
    Ok(empty_command("CREATE MATERIALIZED PROJECTION"))
}

pub(super) fn refresh_materialized_projection(
    cassie: &Cassie,
    name: &str,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let mut metadata = cassie
        .catalog
        .get_materialized_projection(name)
        .ok_or_else(|| {
            QueryError::General(format!("materialized projection '{name}' does not exist"))
        })?;
    set_projection_building(cassie, &mut metadata)?;

    match build_active_version(cassie, &mut metadata, user_functions, controls) {
        Ok(()) => {
            cassie
                .runtime
                .record_materialized_projection_refresh(metadata.collection.clone());
            persist_projection_metadata(cassie, metadata)?;
            Ok(empty_command("REFRESH MATERIALIZED PROJECTION"))
        }
        Err(error) => {
            metadata.freshness = catalog::ProjectionFreshness::Failed;
            metadata.rebuild_state = catalog::ProjectionRebuildState::Failed;
            metadata.last_error = Some(error.to_string());
            if let Some(materialized) = metadata.materialized.as_mut() {
                materialized.state = catalog::MaterializedProjectionState::Failed;
            }
            let _ = persist_projection_metadata(cassie, metadata);
            Err(error)
        }
    }
}

pub(super) fn drop_materialized_projection(
    cassie: &Cassie,
    name: &str,
    if_exists: bool,
) -> Result<QueryResult, QueryError> {
    let Some(metadata) = cassie.catalog.get_materialized_projection(name) else {
        if if_exists {
            return Ok(empty_command("DROP MATERIALIZED PROJECTION"));
        }
        return Err(QueryError::General(format!(
            "materialized projection '{name}' does not exist"
        )));
    };

    for version in &metadata.versions {
        let _ = cassie.midge.drop_collection(&version.output_collection);
        cassie
            .catalog
            .unregister_collection(&version.output_collection);
    }
    cassie
        .midge
        .delete_projection_metadata(name)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.unregister_projection_metadata(name);
    Ok(empty_command("DROP MATERIALIZED PROJECTION"))
}

pub(super) fn alter_materialized_projection(
    cassie: &Cassie,
    statement: &crate::sql::ast::AlterMaterializedProjectionStatement,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    match &statement.operation {
        crate::sql::ast::AlterMaterializedProjectionOperation::BuildVersion => {
            build_projection_version(cassie, &statement.name, user_functions, controls)
        }
        crate::sql::ast::AlterMaterializedProjectionOperation::ActivateVersion {
            version_id,
            unsafe_override,
        } => activate_projection_version(cassie, &statement.name, version_id, *unsafe_override),
    }
}

pub(super) fn drop_materialized_projection_version(
    cassie: &Cassie,
    name: &str,
    version_id: &str,
) -> Result<QueryResult, QueryError> {
    let mut metadata = cassie
        .catalog
        .get_materialized_projection(name)
        .ok_or_else(|| {
            QueryError::General(format!("materialized projection '{name}' does not exist"))
        })?;
    if metadata.active_version.as_deref() == Some(version_id) {
        return Err(QueryError::General(format!(
            "cannot drop active projection version '{version_id}'"
        )));
    }
    let Some(index) = metadata
        .versions
        .iter()
        .position(|version| version.version_id == version_id)
    else {
        return Err(QueryError::General(format!(
            "projection version '{version_id}' does not exist"
        )));
    };
    let version = metadata.versions.remove(index);
    let _ = cassie.midge.drop_collection(&version.output_collection);
    cassie
        .catalog
        .unregister_collection(&version.output_collection);
    persist_projection_metadata(cassie, metadata)?;
    Ok(empty_command("DROP MATERIALIZED PROJECTION VERSION"))
}

fn build_projection_version(
    cassie: &Cassie,
    name: &str,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let mut metadata = cassie
        .catalog
        .get_materialized_projection(name)
        .ok_or_else(|| {
            QueryError::General(format!("materialized projection '{name}' does not exist"))
        })?;
    let materialized = metadata.materialized.clone().ok_or_else(|| {
        QueryError::General(format!(
            "materialized projection '{name}' is missing definition"
        ))
    })?;
    let next_ordinal = metadata.versions.len() + 1;
    let version_id = format!("v{next_ordinal}");
    let output_collection = catalog::materialized_output_collection(name, &version_id);
    metadata.versions.push(catalog::ProjectionVersionMeta {
        version_id: version_id.clone(),
        output_collection,
        definition_fingerprint: materialized.definition_fingerprint,
        source_schema_epoch: cassie.catalog.version(),
        state: catalog::ProjectionVersionState::Building,
        created_ms: now_ms(),
        activated_ms: None,
        retired_ms: None,
        last_error: None,
    });
    persist_projection_metadata(cassie, metadata.clone())?;

    match build_specific_version(cassie, &mut metadata, &version_id, user_functions, controls) {
        Ok(()) => {
            persist_projection_metadata(cassie, metadata)?;
            Ok(empty_command("ALTER MATERIALIZED PROJECTION"))
        }
        Err(error) => {
            if let Some(version) = metadata
                .versions
                .iter_mut()
                .find(|version| version.version_id == version_id)
            {
                version.state = catalog::ProjectionVersionState::Failed;
                version.last_error = Some(error.to_string());
            }
            metadata.last_error = Some(error.to_string());
            let _ = persist_projection_metadata(cassie, metadata);
            Err(error)
        }
    }
}

fn activate_projection_version(
    cassie: &Cassie,
    name: &str,
    version_id: &str,
    unsafe_override: bool,
) -> Result<QueryResult, QueryError> {
    let mut metadata = cassie
        .catalog
        .get_materialized_projection(name)
        .ok_or_else(|| {
            QueryError::General(format!("materialized projection '{name}' does not exist"))
        })?;
    let Some(target_index) = metadata
        .versions
        .iter()
        .position(|version| version.version_id == version_id)
    else {
        return Err(QueryError::General(format!(
            "projection version '{version_id}' does not exist"
        )));
    };
    let target_state = metadata.versions[target_index].state.clone();
    if !matches!(
        target_state,
        catalog::ProjectionVersionState::Built | catalog::ProjectionVersionState::Active
    ) {
        return Err(QueryError::General(format!(
            "projection version '{version_id}' is not built"
        )));
    }

    let previous = metadata.active_version.clone();
    for version in &mut metadata.versions {
        if version.version_id == version_id {
            version.state = catalog::ProjectionVersionState::Active;
            version.activated_ms = Some(now_ms());
        } else if Some(&version.version_id) == previous.as_ref() {
            version.state = catalog::ProjectionVersionState::Retired;
            version.retired_ms = Some(now_ms());
        }
    }
    metadata.active_version = Some(version_id.to_string());
    metadata.freshness = catalog::ProjectionFreshness::Fresh;
    metadata.swap = catalog::ProjectionSwapMeta {
        target_version_id: Some(version_id.to_string()),
        previous_version_id: previous,
        swapped_at_ms: Some(now_ms()),
        unsafe_override,
        last_error: None,
    };
    persist_projection_metadata(cassie, metadata)?;
    cassie.runtime.record_projection_swap(name.to_string());
    Ok(empty_command("ALTER MATERIALIZED PROJECTION"))
}

fn build_active_version(
    cassie: &Cassie,
    metadata: &mut catalog::ProjectionMeta,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    let active = metadata
        .active_version
        .clone()
        .or_else(|| {
            metadata
                .versions
                .first()
                .map(|version| version.version_id.clone())
        })
        .ok_or_else(|| QueryError::General("projection has no versions".into()))?;
    build_specific_version(cassie, metadata, &active, user_functions, controls)?;
    activate_built_version(metadata, &active);
    Ok(())
}

fn build_specific_version(
    cassie: &Cassie,
    metadata: &mut catalog::ProjectionMeta,
    version_id: &str,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    let materialized = metadata.materialized.clone().ok_or_else(|| {
        QueryError::General(format!(
            "materialized projection '{}' is missing definition",
            metadata.collection
        ))
    })?;
    let build = plan_projection_query(cassie, &materialized.query, user_functions)?;
    let output_collection = metadata
        .versions
        .iter()
        .find(|version| version.version_id == version_id)
        .map(|version| version.output_collection.clone())
        .ok_or_else(|| {
            QueryError::General(format!("projection version '{version_id}' does not exist"))
        })?;
    let rows = execute_plan(
        cassie,
        None,
        &build.logical,
        &mut CteContext::new(),
        user_functions,
        &[],
        controls,
    )?;
    replace_output_rows(cassie, &output_collection, &build.schema, rows)?;
    cassie
        .runtime
        .record_materialized_projection_build(metadata.collection.clone());

    if let Some(version) = metadata
        .versions
        .iter_mut()
        .find(|version| version.version_id == version_id)
    {
        version.state = catalog::ProjectionVersionState::Built;
        version.last_error = None;
    }
    if let Some(materialized) = metadata.materialized.as_mut() {
        materialized.output_schema = build.schema;
        materialized.state = catalog::MaterializedProjectionState::Ready;
        materialized.last_built_ms = Some(now_ms());
    }
    metadata.rebuild_state = catalog::ProjectionRebuildState::Idle;
    metadata.freshness = catalog::ProjectionFreshness::Fresh;
    metadata.last_error = None;
    metadata.lag = 0;
    Ok(())
}

fn activate_built_version(metadata: &mut catalog::ProjectionMeta, version_id: &str) {
    let previous = metadata.active_version.clone();
    for version in &mut metadata.versions {
        if version.version_id == version_id {
            version.state = catalog::ProjectionVersionState::Active;
            version.activated_ms = Some(now_ms());
        } else if Some(&version.version_id) == previous.as_ref() {
            version.state = catalog::ProjectionVersionState::Retired;
            version.retired_ms = Some(now_ms());
        }
    }
    metadata.active_version = Some(version_id.to_string());
}

fn set_projection_building(
    cassie: &Cassie,
    metadata: &mut catalog::ProjectionMeta,
) -> Result<(), QueryError> {
    metadata.freshness = catalog::ProjectionFreshness::Rebuilding;
    metadata.rebuild_state = catalog::ProjectionRebuildState::Rebuilding;
    if let Some(materialized) = metadata.materialized.as_mut() {
        materialized.state = catalog::MaterializedProjectionState::Building;
    }
    persist_projection_metadata(cassie, metadata.clone())
}

fn persist_projection_metadata(
    cassie: &Cassie,
    metadata: catalog::ProjectionMeta,
) -> Result<(), QueryError> {
    cassie
        .midge
        .put_projection_metadata(metadata.clone())
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_projection_metadata(metadata);
    Ok(())
}

struct ProjectionBuildPlan {
    logical: LogicalPlan,
    select: crate::sql::ast::SelectStatement,
    schema: Schema,
    source_collections: Vec<String>,
}

fn plan_projection_query(
    cassie: &Cassie,
    query: &str,
    _user_functions: &HashMap<String, FunctionMeta>,
) -> Result<ProjectionBuildPlan, QueryError> {
    let parsed =
        crate::sql::parser::parse_statement(query).map_err(|error| QueryError::General(error.0))?;
    if crate::sql::parameter_count(&parsed) != 0 {
        return Err(QueryError::General(
            "materialized projection definitions cannot contain bind parameters".into(),
        ));
    }
    let bound = crate::sql::binder::bind(parsed, &cassie.catalog)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let QueryStatement::Select(select) = &bound.statement.statement else {
        return Err(QueryError::General(
            "materialized projection definition must be a SELECT".into(),
        ));
    };
    let schema = crate::sql::binder::infer_select_schema(select, &cassie.catalog)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let source_collections = collect_source_collections(&select.source);
    let logical = crate::planner::logical::plan(&bound)
        .map_err(|error| QueryError::General(error.to_string()))?;
    Ok(ProjectionBuildPlan {
        logical,
        select: select.clone(),
        schema,
        source_collections,
    })
}

fn reject_unsupported_sources(cassie: &Cassie, sources: &[String]) -> Result<(), QueryError> {
    if sources.is_empty() {
        return Err(QueryError::General(
            "materialized projection requires at least one source collection".into(),
        ));
    }
    for source in sources {
        if cassie.catalog.is_materialized_projection(source) {
            return Err(QueryError::General(
                "recursive materialized projections are not supported".into(),
            ));
        }
        if virtual_views::schema(source).is_some() {
            return Err(QueryError::General(
                "materialized projections over virtual catalog views are not supported".into(),
            ));
        }
    }
    Ok(())
}

fn reject_nondeterministic_select(
    select: &crate::sql::ast::SelectStatement,
    user_functions: &HashMap<String, FunctionMeta>,
) -> Result<(), QueryError> {
    for function in functions_in_select(select) {
        let name = function.to_ascii_lowercase();
        if matches!(
            name.as_str(),
            "version"
                | "current_schema"
                | "current_database"
                | "current_user"
                | "session_user"
                | "current_role"
                | "search"
                | "search_score"
                | "hybrid_score"
                | "snippet"
        ) {
            return Err(QueryError::General(format!(
                "non-deterministic function '{function}' is not supported in materialized projections"
            )));
        }
        if user_functions
            .get(&name)
            .is_some_and(|function| function.volatility == catalog::Volatility::Volatile)
        {
            return Err(QueryError::General(format!(
                "volatile function '{function}' is not supported in materialized projections"
            )));
        }
    }
    Ok(())
}

fn replace_output_rows(
    cassie: &Cassie,
    output_collection: &str,
    schema: &Schema,
    rows: Vec<BatchRow>,
) -> Result<(), QueryError> {
    if cassie.midge.collection_schema(output_collection).is_some() {
        let _ = cassie.midge.drop_collection(output_collection);
        cassie.catalog.unregister_collection(output_collection);
    }
    cassie
        .midge
        .create_collection(output_collection, schema.clone())
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_collection(
        output_collection,
        schema
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.data_type.clone()))
            .collect(),
    );

    for (index, row) in rows.into_iter().enumerate() {
        let entries = row.into_entries();
        let payload = serde_json::Value::Object(
            entries
                .iter()
                .map(|(name, value)| (name.clone(), value_to_json(value.clone())))
                .collect(),
        );
        let id = deterministic_row_id(index, &payload);
        cassie
            .midge
            .put_document(output_collection, Some(id), payload)
            .map_err(|error| QueryError::General(error.to_string()))?;
    }
    Ok(())
}

fn value_to_json(value: Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(value) => serde_json::Value::Bool(value),
        Value::Int64(value) => serde_json::Value::Number(value.into()),
        Value::Float64(value) => serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::String(value) => serde_json::Value::String(value),
        Value::Vector(value) => serde_json::json!(value.values),
        Value::Json(value) => value,
    }
}

fn collect_source_collections(source: &QuerySource) -> Vec<String> {
    let mut out = Vec::new();
    collect_source_collections_into(source, &mut out);
    out.sort();
    out.dedup();
    out
}

fn collect_source_collections_into(source: &QuerySource, out: &mut Vec<String>) {
    match source {
        QuerySource::Collection(name) => out.push(name.clone()),
        QuerySource::Subquery { select, .. } => {
            collect_source_collections_into(&select.source, out)
        }
        QuerySource::Join { left, right, .. } => {
            collect_source_collections_into(left, out);
            collect_source_collections_into(right, out);
        }
        QuerySource::Cte(_) | QuerySource::SingleRow => {}
    }
}

fn functions_in_select(select: &crate::sql::ast::SelectStatement) -> Vec<String> {
    let mut out = Vec::new();
    for item in &select.projection {
        collect_functions_in_select_item(item, &mut out);
    }
    if let Some(filter) = &select.filter {
        collect_functions_in_expr(filter, &mut out);
    }
    for expr in &select.group_by {
        collect_functions_in_expr(expr, &mut out);
    }
    if let Some(having) = &select.having {
        collect_functions_in_expr(having, &mut out);
    }
    for order in &select.order {
        collect_functions_in_expr(&order.expr, &mut out);
    }
    if let Some(set) = &select.set {
        out.extend(functions_in_select(&set.right));
    }
    out
}

fn collect_functions_in_select_item(item: &SelectItem, out: &mut Vec<String>) {
    match item {
        SelectItem::Function { function, .. } => collect_functions_in_call(function, out),
        SelectItem::Expr { expr, .. } => collect_functions_in_expr(expr, out),
        SelectItem::WindowFunction { function, .. } => {
            out.push(function.name.clone());
            for arg in &function.args {
                collect_functions_in_expr(arg, out);
            }
            for expr in &function.partition_by {
                collect_functions_in_expr(expr, out);
            }
            for order in &function.order_by {
                collect_functions_in_expr(&order.expr, out);
            }
        }
        SelectItem::Wildcard | SelectItem::Column { .. } => {}
    }
}

fn collect_functions_in_expr(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Function(function) => collect_functions_in_call(function, out),
        Expr::Binary { left, right, .. } => {
            collect_functions_in_expr(left, out);
            collect_functions_in_expr(right, out);
        }
        Expr::Not { expr } | Expr::Cast { expr, .. } | Expr::IsNull { expr, .. } => {
            collect_functions_in_expr(expr, out)
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            collect_functions_in_expr(expr, out);
            collect_functions_in_expr(low, out);
            collect_functions_in_expr(high, out);
        }
        Expr::InList { expr, values, .. } => {
            collect_functions_in_expr(expr, out);
            for value in values {
                collect_functions_in_expr(value, out);
            }
        }
        Expr::Exists(statement) => {
            if let QueryStatement::Select(select) = &statement.statement {
                out.extend(functions_in_select(select));
            }
        }
        Expr::Column(_)
        | Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null => {}
    }
}

fn collect_functions_in_call(function: &FunctionCall, out: &mut Vec<String>) {
    out.push(function.name.clone());
    for arg in &function.args {
        collect_functions_in_expr(arg, out);
    }
}

fn deterministic_row_id(index: usize, payload: &serde_json::Value) -> String {
    format!("{:016x}-{index:016x}", fnv1a64(&payload.to_string()))
}

fn stable_projection_fingerprint(query: &str) -> u64 {
    fnv1a64(&query.to_ascii_lowercase())
}

fn fnv1a64(input: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn empty_command(command: &str) -> QueryResult {
    QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
        command: command.to_string(),
    }
}
