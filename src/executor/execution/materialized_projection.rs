#[path = "materialized_projection_verification.rs"]
mod verification;
pub(crate) use verification::projection_verification_column_definitions;
pub(super) use verification::{verify_projection, verify_projection_state};

pub(super) use super::command_output::empty as empty_command;
#[path = "materialized_projection/read_equivalence.rs"]
mod read_equivalence;
pub(super) use read_equivalence::preserves_base_columns;

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{
    catalog, execute_plan, virtual_views, BatchRow, Cassie, CteContext, Expr, FunctionCall,
    FunctionMeta, LogicalPlan, QueryError, QueryExecutionControls, QueryResult, QuerySource,
    QueryStatement, Schema, SelectItem,
};
use crate::midge::adapter::RootHashRecord;
use crate::types::json::try_value_to_json;

pub(super) use super::materialized_projection_maintenance::mark_source_projections_stale;

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

pub(super) fn create_materialized_projection(
    cassie: &Cassie,
    session: Option<&crate::app::CassieSession>,
    statement: &crate::sql::ast::CreateMaterializedProjectionStatement,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    create_materialized_relation(
        cassie,
        session,
        MaterializedRelationSpec {
            name: &statement.name,
            if_not_exists: statement.if_not_exists,
            query: &statement.query,
            options: &statement.options,
            output_columns: &[],
            with_data: true,
            initial_materialization: InitialMaterialization::AfterCatalogPublish,
            require_source_collection: true,
            command_tag: "CREATE MATERIALIZED PROJECTION",
        },
        user_functions,
        controls,
    )
}

pub(super) fn create_materialized_view(
    cassie: &Cassie,
    session: Option<&crate::app::CassieSession>,
    statement: &crate::sql::ast::CreateMaterializedViewStatement,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let mut options = std::collections::BTreeMap::new();
    options.insert(
        catalog::SQL_MATERIALIZED_VIEW_OPTION.to_string(),
        "true".to_string(),
    );
    create_materialized_relation(
        cassie,
        session,
        MaterializedRelationSpec {
            name: &statement.name,
            if_not_exists: statement.if_not_exists,
            query: &statement.query,
            options: &options,
            output_columns: &statement.columns,
            with_data: statement.with_data,
            initial_materialization: InitialMaterialization::BeforeCatalogPublish,
            require_source_collection: false,
            command_tag: "CREATE MATERIALIZED VIEW",
        },
        user_functions,
        controls,
    )
}

#[derive(Clone, Copy)]
enum InitialMaterialization {
    BeforeCatalogPublish,
    AfterCatalogPublish,
}

#[derive(Clone, Copy)]
struct MaterializedRelationSpec<'a> {
    name: &'a str,
    if_not_exists: bool,
    query: &'a str,
    options: &'a std::collections::BTreeMap<String, String>,
    output_columns: &'a [String],
    with_data: bool,
    initial_materialization: InitialMaterialization,
    require_source_collection: bool,
    command_tag: &'a str,
}

fn create_materialized_relation(
    cassie: &Cassie,
    session: Option<&crate::app::CassieSession>,
    spec: MaterializedRelationSpec<'_>,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let MaterializedRelationSpec {
        name,
        if_not_exists,
        query,
        options,
        output_columns,
        with_data,
        initial_materialization,
        require_source_collection,
        command_tag,
    } = spec;
    if if_not_exists && cassie.catalog.relation_exists(name) {
        return empty_command(command_tag, controls);
    }
    if cassie.catalog.relation_exists(name) || virtual_views::schema(name).is_some() {
        return Err(QueryError::General(format!(
            "relation '{name}' already exists"
        )));
    }

    let context = projection_binding_context(cassie, session, Some(name));
    let mut build = plan_projection_query(cassie, query, user_functions, &context)?;
    reject_unsupported_sources(cassie, &build.source_collections, require_source_collection)?;
    reject_nondeterministic_select(&build.select, user_functions)?;
    rename_output_schema(&mut build.schema, output_columns)?;

    let mut metadata = catalog::ProjectionMeta::materialized(catalog::MaterializedProjectionSpec {
        name: name.to_string(),
        query: query.to_string(),
        source_collections: build.source_collections,
        output_schema: build.schema,
        options: options.clone(),
        schema_epoch: cassie.catalog.version(),
        definition_fingerprint: stable_projection_fingerprint(query),
        created_ms: now_ms(),
    });
    let output = empty_command(command_tag, controls)?;
    let created = cassie.midge.with_collection_gates(
        &[name.to_string()],
        || -> Result<bool, QueryError> {
            if if_not_exists && cassie.catalog.relation_exists(name) {
                return Ok(false);
            }
            if cassie.catalog.relation_exists(name) || virtual_views::schema(name).is_some() {
                return Err(QueryError::General(format!(
                    "relation '{name}' already exists"
                )));
            }

            if with_data
                && matches!(
                    initial_materialization,
                    InitialMaterialization::BeforeCatalogPublish
                )
            {
                if let Err(error) =
                    build_active_version(cassie, &mut metadata, user_functions, controls)
                {
                    discard_unpublished_output(cassie, &metadata);
                    return Err(error);
                }
            }

            if let Err(error) = cassie.midge.put_projection_metadata(&metadata) {
                if with_data
                    && matches!(
                        initial_materialization,
                        InitialMaterialization::BeforeCatalogPublish
                    )
                {
                    discard_unpublished_output(cassie, &metadata);
                }
                return Err(QueryError::General(error.to_string()));
            }
            cassie.catalog.register_projection_metadata(metadata);
            Ok(true)
        },
    )?;
    if !created {
        return Ok(output);
    }
    if with_data
        && matches!(
            initial_materialization,
            InitialMaterialization::AfterCatalogPublish
        )
    {
        refresh_materialized_projection_data(cassie, session, name, user_functions, controls)?;
    }
    Ok(output)
}

fn discard_unpublished_output(cassie: &Cassie, metadata: &catalog::ProjectionMeta) {
    for version in &metadata.versions {
        if cassie
            .midge
            .collection_schema(&version.output_collection)
            .is_some()
        {
            if let Err(error) = cassie.midge.drop_collection(&version.output_collection) {
                tracing::warn!(
                    collection = version.output_collection.as_str(),
                    error = error.to_string().as_str(),
                    "failed to discard an unpublished materialized-view output collection"
                );
            }
        }
        let _ = cassie
            .catalog
            .unregister_collection(&version.output_collection);
    }
}

fn rename_output_schema(schema: &mut Schema, column_names: &[String]) -> Result<(), QueryError> {
    if column_names.is_empty() {
        return Ok(());
    }
    if column_names.len() != schema.fields.len() {
        return Err(QueryError::General(format!(
            "materialized-view column list has {} names for {} query columns",
            column_names.len(),
            schema.fields.len()
        )));
    }
    let mut seen = std::collections::HashSet::with_capacity(column_names.len());
    for (field, name) in schema.fields.iter_mut().zip(column_names) {
        let key = name.to_ascii_lowercase();
        if !seen.insert(key) {
            return Err(QueryError::General(format!(
                "materialized-view column '{name}' specified more than once"
            )));
        }
        field.name.clone_from(name);
    }
    Ok(())
}

pub(super) fn refresh_materialized_projection(
    cassie: &Cassie,
    session: Option<&crate::app::CassieSession>,
    name: &str,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let output = empty_command("REFRESH MATERIALIZED PROJECTION", controls)?;
    refresh_materialized_projection_data(cassie, session, name, user_functions, controls)?;
    Ok(output)
}

pub(super) fn refresh_materialized_view(
    cassie: &Cassie,
    session: Option<&crate::app::CassieSession>,
    statement: &crate::sql::ast::RefreshMaterializedViewStatement,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let output = empty_command("REFRESH MATERIALIZED VIEW", controls)?;
    require_sql_materialized_view(cassie, &statement.name)?;
    if statement.with_data {
        refresh_materialized_projection_data(
            cassie,
            session,
            &statement.name,
            user_functions,
            controls,
        )?;
    } else {
        clear_materialized_view_data(cassie, &statement.name)?;
    }
    Ok(output)
}

fn clear_materialized_view_data(cassie: &Cassie, name: &str) -> Result<(), QueryError> {
    let mut metadata = sql_materialized_view_metadata(cassie, name)?;
    if let Some(active) = metadata.active_version.take() {
        if let Some(version) = metadata
            .versions
            .iter_mut()
            .find(|version| version.version_id == active)
        {
            version.state = catalog::ProjectionVersionState::Retired;
            version.retired_ms = Some(now_ms());
        }
    }
    metadata.freshness = catalog::ProjectionFreshness::Unknown;
    metadata.rebuild_state = catalog::ProjectionRebuildState::Idle;
    metadata.verification = catalog::ProjectionRebuildVerificationMeta::default();
    if let Some(materialized) = metadata.materialized.as_mut() {
        materialized.state = catalog::MaterializedProjectionState::Building;
    }
    persist_projection_metadata(cassie, metadata)
}

pub(super) fn refresh_materialized_projection_data(
    cassie: &Cassie,
    _session: Option<&crate::app::CassieSession>,
    name: &str,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
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
            Ok(())
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
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    drop_materialized_relation(
        cassie,
        name,
        if_exists,
        "DROP MATERIALIZED PROJECTION",
        controls,
    )
}

pub(super) fn drop_materialized_view(
    cassie: &Cassie,
    statement: &crate::sql::ast::DropMaterializedViewStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    if !is_sql_materialized_view(cassie, &statement.name) {
        if statement.if_exists {
            return empty_command("DROP MATERIALIZED VIEW", controls);
        }
        return Err(QueryError::General(format!(
            "materialized view '{}' does not exist",
            statement.name
        )));
    }
    drop_materialized_relation(
        cassie,
        &statement.name,
        statement.if_exists,
        "DROP MATERIALIZED VIEW",
        controls,
    )
}

fn require_sql_materialized_view(cassie: &Cassie, name: &str) -> Result<(), QueryError> {
    if is_sql_materialized_view(cassie, name) {
        Ok(())
    } else {
        Err(QueryError::General(format!(
            "materialized view '{name}' does not exist"
        )))
    }
}

fn is_sql_materialized_view(cassie: &Cassie, name: &str) -> bool {
    cassie
        .catalog
        .get_materialized_projection(name)
        .is_some_and(|projection| {
            projection
                .materialized
                .as_ref()
                .is_some_and(catalog::MaterializedProjectionMeta::is_sql_materialized_view)
        })
}

fn sql_materialized_view_metadata(
    cassie: &Cassie,
    name: &str,
) -> Result<catalog::ProjectionMeta, QueryError> {
    cassie
        .catalog
        .get_materialized_projection(name)
        .filter(|projection| {
            projection
                .materialized
                .as_ref()
                .is_some_and(catalog::MaterializedProjectionMeta::is_sql_materialized_view)
        })
        .ok_or_else(|| QueryError::General(format!("materialized view '{name}' does not exist")))
}

fn drop_materialized_relation(
    cassie: &Cassie,
    name: &str,
    if_exists: bool,
    command_tag: &str,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let Some(metadata) = cassie.catalog.get_materialized_projection(name) else {
        if if_exists {
            return empty_command(command_tag, controls);
        }
        return Err(QueryError::General(format!(
            "materialized relation '{name}' does not exist"
        )));
    };

    let output = empty_command(command_tag, controls)?;
    // Storage and catalog removal use exact keys, so drop the stored projection
    // name rather than the name as it was typed.
    let projection = metadata.collection.clone();
    cassie
        .midge
        .with_collection_gates(std::slice::from_ref(&projection), || {
            drop_materialized_projection_gated(cassie, &projection, &metadata)
        })?;
    Ok(output)
}

/// Removes one materialized projection while holding its catalog gate, so the
/// reports read from the catalog cannot change before they are deleted.
///
/// Stored reports go first: if their deletion fails, the projection, its
/// versions, and its outputs are all still intact. Leftover output collections
/// are recoverable, so they are dropped last and their failures are reported.
fn drop_materialized_projection_gated(
    cassie: &Cassie,
    projection: &str,
    metadata: &catalog::ProjectionMeta,
) -> Result<(), QueryError> {
    let repair_reports = cassie.catalog.projection_repair_report_ids(projection);
    let comparison_reports = cassie.catalog.projection_comparison_report_ids(projection);
    cassie
        .midge
        .delete_projection_reports(&repair_reports, &comparison_reports)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie
        .midge
        .delete_projection_metadata(projection)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie
        .catalog
        .unregister_projection_repair_reports(&repair_reports);
    cassie
        .catalog
        .unregister_projection_comparison_reports(&comparison_reports);
    cassie.catalog.unregister_projection_metadata(projection);
    for version in &metadata.versions {
        if let Err(error) = cassie.midge.drop_collection(&version.output_collection) {
            tracing::warn!(
                collection = version.output_collection.as_str(),
                error = error.to_string().as_str(),
                "dropping a materialized projection left its version output collection behind"
            );
        }
        let _ = cassie
            .catalog
            .unregister_collection(&version.output_collection);
    }
    Ok(())
}

pub(super) fn alter_materialized_projection(
    cassie: &Cassie,
    statement: &crate::sql::ast::AlterMaterializedProjectionStatement,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    match &statement.operation {
        crate::sql::ast::AlterMaterializedProjectionOperation::BuildVersion => {
            super::materialized_projection_versions::build_projection_version(
                cassie,
                &statement.name,
                user_functions,
                controls,
            )
        }
        crate::sql::ast::AlterMaterializedProjectionOperation::ActivateVersion {
            version_id,
            unsafe_override,
        } => activate_projection_version(
            cassie,
            &statement.name,
            version_id,
            *unsafe_override,
            controls,
        ),
    }
}

fn activate_projection_version(
    cassie: &Cassie,
    name: &str,
    version_id: &str,
    unsafe_override: bool,
    controls: &QueryExecutionControls,
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
    let target_verification = metadata.versions[target_index].verification.state.clone();
    if !unsafe_override
        && !matches!(
            target_verification,
            catalog::ProjectionVerificationState::Verified
                | catalog::ProjectionVerificationState::Skipped
                | catalog::ProjectionVerificationState::Unknown
        )
    {
        metadata.swap = catalog::ProjectionSwapMeta {
            target_version_id: Some(version_id.to_string()),
            previous_version_id: metadata.active_version.clone(),
            swapped_at_ms: None,
            unsafe_override: false,
            last_error: Some(format!(
                "projection version '{version_id}' verification state is {}",
                target_verification.as_str()
            )),
        };
        persist_projection_metadata(cassie, metadata)?;
        return Err(QueryError::General(format!(
            "projection version '{version_id}' is not verified"
        )));
    }

    let output = empty_command("ALTER MATERIALIZED PROJECTION", controls)?;
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
    crate::executor::check_projection_activation_failure_point()
        .map_err(|error| QueryError::General(error.to_string()))?;
    persist_projection_metadata(cassie, metadata)?;
    cassie.runtime.record_projection_swap(name.to_string());
    cassie
        .runtime
        .record_projection_activation_write(name.to_string());
    Ok(output)
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

pub(super) fn build_specific_version(
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
    let context = projection_binding_context(cassie, None, Some(&metadata.collection));
    let mut build = plan_projection_query(cassie, &materialized.query, user_functions, &context)?;
    let output_columns = materialized
        .output_schema
        .fields
        .iter()
        .map(|field| field.name.clone())
        .collect::<Vec<_>>();
    rename_output_schema(&mut build.schema, &output_columns)?;
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
    metadata.source_generations = materialized
        .source_collections
        .iter()
        .filter_map(|source| {
            cassie
                .midge
                .collection_generation(source)
                .ok()
                .map(|generation| (source.clone(), generation))
        })
        .collect();
    let root = replace_output_rows(cassie, &output_collection, &build.schema, rows)?;
    cassie
        .runtime
        .record_materialized_projection_build(metadata.collection.clone());
    let verification = verify_rebuilt_output(
        &metadata.collection,
        version_id,
        materialized.definition_fingerprint,
        &root,
    );

    if let Some(version) = metadata
        .versions
        .iter_mut()
        .find(|version| version.version_id == version_id)
    {
        version.state = catalog::ProjectionVersionState::Built;
        version.last_error = None;
        version.verification = verification.clone();
    }
    if let Some(materialized) = metadata.materialized.as_mut() {
        materialized.output_schema = build.schema;
        materialized.state = catalog::MaterializedProjectionState::Ready;
        materialized.last_built_ms = Some(now_ms());
    }
    metadata.rebuild_state = catalog::ProjectionRebuildState::Idle;
    metadata.freshness = catalog::ProjectionFreshness::Fresh;
    metadata.hashes = cassie
        .midge
        .projection_hash_summary(&output_collection)
        .map_err(|error| QueryError::General(error.to_string()))?
        .unwrap_or_default();
    metadata.hashes.root.projection_version_id = Some(version_id.to_string());
    metadata.verification = verification;
    metadata.last_error = None;
    metadata.lag = 0;
    cassie.runtime.record_projection_rebuild_verification(
        metadata.collection.clone(),
        metadata.verification.state != catalog::ProjectionVerificationState::Verified,
    );
    Ok(())
}

fn verify_rebuilt_output(
    projection: &str,
    version_id: &str,
    definition_fingerprint: u64,
    root: &crate::midge::adapter::RootHashRecord,
) -> catalog::ProjectionRebuildVerificationMeta {
    let completed_ms = Some(now_ms());
    let mut metadata = catalog::ProjectionRebuildVerificationMeta {
        state: catalog::ProjectionVerificationState::Verified,
        started_ms: completed_ms,
        completed_ms,
        mismatch_count: 0,
        unverifiable_ranges: 0,
        failure_reason: None,
    };
    if root.algorithm != "cassie-fnv128" || root.digest_length != 16 {
        metadata.state = catalog::ProjectionVerificationState::Incompatible;
        metadata.failure_reason = Some("incompatible projection root hash metadata".to_string());
    } else if root.state == crate::midge::adapter::StoredHashState::Incomplete
        || root.state == crate::midge::adapter::StoredHashState::Stale
    {
        metadata.state = catalog::ProjectionVerificationState::Failed;
        metadata.unverifiable_ranges = root.range_count;
        metadata.failure_reason = Some("projection root hash is not current".to_string());
    } else if definition_fingerprint == 0 || projection.is_empty() || version_id.is_empty() {
        metadata.state = catalog::ProjectionVerificationState::Unverifiable;
        metadata.failure_reason = Some("projection target identity is incomplete".to_string());
    }
    metadata
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
    metadata.verification.state = catalog::ProjectionVerificationState::Pending;
    if let Some(materialized) = metadata.materialized.as_mut() {
        materialized.state = catalog::MaterializedProjectionState::Building;
    }
    persist_projection_metadata(cassie, metadata.clone())
}

pub(super) fn persist_projection_metadata(
    cassie: &Cassie,
    metadata: catalog::ProjectionMeta,
) -> Result<(), QueryError> {
    cassie
        .midge
        .put_projection_metadata(&metadata)
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
    context: &crate::sql::binder::BindingContext,
) -> Result<ProjectionBuildPlan, QueryError> {
    let parsed = crate::sql::parser::parse_statement(query)
        .map_err(|error| QueryError::General(error.to_string()))?;
    crate::sql::definition_guard::statement(&parsed).map_err(QueryError::Cassie)?;
    if crate::sql::parameter_count(&parsed) != 0 {
        return Err(QueryError::General(
            "materialized projection definitions cannot contain bind parameters".into(),
        ));
    }
    let bound = crate::sql::binder::bind_with_context(parsed, &cassie.catalog, context)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let QueryStatement::Select(select) = &bound.statement.statement else {
        return Err(QueryError::General(
            "materialized projection definition must be a SELECT".into(),
        ));
    };
    let schema =
        crate::sql::binder::infer_select_schema_with_context(select, &cassie.catalog, context)
            .map_err(|error| QueryError::General(error.to_string()))?;
    let source_collections = collect_source_collections(&select.source);
    let mut logical = crate::planner::logical::plan(&bound)
        .map_err(|error| QueryError::General(error.to_string()))?;
    crate::planner::logical::rewrite_reserved_id_references(&mut logical, &cassie.catalog);
    Ok(ProjectionBuildPlan {
        logical,
        select: select.as_ref().clone(),
        schema,
        source_collections,
    })
}

fn projection_binding_context(
    cassie: &Cassie,
    session: Option<&crate::app::CassieSession>,
    projection_name: Option<&str>,
) -> crate::sql::binder::BindingContext {
    let default_database = cassie.default_database.clone();
    let default_path = vec![crate::catalog::DEFAULT_SCHEMA.to_string()];
    if let Some(session) = session {
        let database = session
            .current_database()
            .unwrap_or(default_database.as_str());
        return if cassie.database_catalog_enforced() {
            crate::sql::binder::BindingContext::scoped(
                database.to_string(),
                cassie.binding_search_path(session),
            )
        } else {
            crate::sql::binder::BindingContext::unscoped(
                database.to_string(),
                cassie.binding_search_path(session),
            )
        };
    }

    if let Some(relation) = projection_name.and_then(crate::catalog::RelationId::parse_canonical) {
        let mut path = vec![relation.schema.clone()];
        if !relation
            .schema
            .eq_ignore_ascii_case(crate::catalog::DEFAULT_SCHEMA)
        {
            path.push(crate::catalog::DEFAULT_SCHEMA.to_string());
        }
        return crate::sql::binder::BindingContext::scoped(relation.database, path);
    }

    if cassie.database_catalog_enforced() {
        crate::sql::binder::BindingContext::scoped(default_database, default_path)
    } else {
        crate::sql::binder::BindingContext::unscoped(default_database, default_path)
    }
}

fn reject_unsupported_sources(
    cassie: &Cassie,
    sources: &[String],
    require_source_collection: bool,
) -> Result<(), QueryError> {
    if require_source_collection && sources.is_empty() {
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
                | "nextval"
                | "currval"
                | "setval"
                | "lastval"
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
) -> Result<RootHashRecord, QueryError> {
    crate::executor::pause_before_materialized_projection_replace();
    let gated_collection = output_collection.to_string();
    cassie
        .midge
        .with_collection_write_gates(std::slice::from_ref(&gated_collection), || {
            replace_output_rows_gated(cassie, output_collection, schema, rows)
        })
}

fn replace_output_rows_gated(
    cassie: &Cassie,
    output_collection: &str,
    schema: &Schema,
    rows: Vec<BatchRow>,
) -> Result<RootHashRecord, QueryError> {
    let output_rows = rows
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let _operator_parent = row.operator_memory();
            let entries = row.into_entries();
            if entries.len() != schema.fields.len() {
                return Err(QueryError::General(format!(
                    "materialized projection produced {} values for {} output columns",
                    entries.len(),
                    schema.fields.len()
                )));
            }
            let payload = entries
                .into_iter()
                .zip(&schema.fields)
                .map(|((_, value), field)| {
                    try_value_to_json(value)
                        .map(|value| (field.name.clone(), value))
                        .map_err(|error| {
                            QueryError::General(format!("materialized projection output {error}"))
                        })
                })
                .collect::<Result<serde_json::Map<_, _>, _>>()?;
            let payload = serde_json::Value::Object(payload);
            let id = deterministic_row_id(index, &payload);
            Ok((id, payload))
        })
        .collect::<Result<Vec<_>, QueryError>>()?;

    if cassie.midge.collection_schema(output_collection).is_some() {
        let _ = cassie.midge.drop_collection(output_collection);
        let _ = cassie.catalog.unregister_collection(output_collection);
        crate::executor::pause_after_materialized_projection_drop();
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

    let (report, root) = cassie
        .midge
        .write_fresh_projection_output_rows(output_collection, output_rows)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.runtime.record_projection_rebuild_writes(
        output_collection.to_string(),
        report.stats.row_puts,
        report.stats.batch_flushes,
    );
    Ok(root)
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
        QuerySource::Aliased { source, .. } => collect_source_collections_into(source, out),
        QuerySource::Collection(name) => out.push(name.to_string()),
        QuerySource::Subquery { select, .. } => {
            collect_source_collections_into(&select.source, out);
        }
        QuerySource::Join { left, right, .. } => {
            collect_source_collections_into(left, out);
            collect_source_collections_into(right, out);
        }
        QuerySource::Cte(_)
        | QuerySource::TableFunction { .. }
        | QuerySource::Values { .. }
        | QuerySource::SingleRow => {}
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
        Expr::Exists(statement) => {
            if let QueryStatement::Select(select) = &statement.statement {
                out.extend(functions_in_select(select));
            }
        }
        Expr::Subquery(subquery) => {
            if let QueryStatement::Select(select) = &subquery.query.statement {
                out.extend(functions_in_select(select));
            }
            expr.for_each_child(|child| collect_functions_in_expr(child, out));
        }
        _ => expr.for_each_child(|child| collect_functions_in_expr(child, out)),
    }
}

fn collect_functions_in_call(function: &FunctionCall, out: &mut Vec<String>) {
    out.push(function.name.clone());
    for arg in function.expressions() {
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
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}
