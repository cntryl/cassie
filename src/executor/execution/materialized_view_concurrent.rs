//! Concurrent SQL materialized-view refresh using private output versions.

use std::collections::HashSet;

use super::{catalog, Cassie, FunctionMeta, QueryError, QueryExecutionControls, QueryResult};

pub(crate) fn refresh_materialized_view_concurrently_external(
    cassie: &Cassie,
    statement: &crate::sql::ast::RefreshMaterializedViewStatement,
    user_functions: &std::collections::HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let output = super::command_output::empty("REFRESH MATERIALIZED VIEW", controls)?;
    if !statement.with_data {
        return Err(unsupported_no_data());
    }
    let name = statement.name.clone();
    cassie
        .midge
        .with_collection_gates(std::slice::from_ref(&name), || {
            refresh_materialized_view_versioned(cassie, &name, user_functions, controls, true)
        })?;
    Ok(output)
}

pub(super) fn refresh_materialized_view_versioned(
    cassie: &Cassie,
    name: &str,
    user_functions: &std::collections::HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
    require_unique_index: bool,
) -> Result<(), QueryError> {
    let current = sql_materialized_view_metadata(cassie, name)?;
    let indexes = cassie.catalog.list_indexes(name);
    if require_unique_index {
        if current.active_version.is_none() {
            return Err(not_ready(format!(
                "cannot refresh unpopulated materialized view '{name}' concurrently"
            )));
        }
        if !indexes.iter().any(is_eligible_unique_index) {
            return Err(not_ready(format!(
                "cannot refresh materialized view '{name}' concurrently without a qualifying unique index"
            )));
        }
    }

    let existing_versions = current
        .versions
        .iter()
        .map(|version| version.version_id.clone())
        .collect::<HashSet<_>>();
    if let Err(error) = super::materialized_projection_versions::build_projection_version(
        cassie,
        name,
        user_functions,
        controls,
    ) {
        discard_new_version_output(cassie, name, &existing_versions);
        return Err(error);
    }

    let built = sql_materialized_view_metadata(cassie, name)?;
    let version = built
        .versions
        .iter()
        .find(|version| !existing_versions.contains(&version.version_id))
        .cloned()
        .ok_or_else(|| {
            QueryError::General(format!(
                "refresh of materialized view '{name}' did not create an output version"
            ))
        })?;
    if version.state != catalog::ProjectionVersionState::Built {
        return Err(QueryError::General(format!(
            "refresh of materialized view '{name}' did not build its output version"
        )));
    }

    for index in indexes {
        let mut replacement = index;
        replacement
            .collection
            .clone_from(&version.output_collection);
        if let Err(error) = cassie.midge.put_index(&replacement) {
            let error = QueryError::from(error);
            discard_output(cassie, &version.output_collection);
            mark_version_failed(cassie, name, &version.version_id, &error.to_string())?;
            return Err(error);
        }
        cassie.catalog.register_index(replacement);
    }

    super::materialized_projection::activate_projection_version(
        cassie,
        name,
        &version.version_id,
        false,
        controls,
    )?;
    cassie
        .runtime
        .record_materialized_projection_refresh(name.to_string());
    Ok(())
}

fn sql_materialized_view_metadata(
    cassie: &Cassie,
    name: &str,
) -> Result<catalog::ProjectionMeta, QueryError> {
    cassie
        .catalog
        .get_materialized_projection(name)
        .filter(|_| cassie.catalog.is_sql_materialized_view(name))
        .ok_or_else(|| QueryError::General(format!("materialized view '{name}' does not exist")))
}

fn is_eligible_unique_index(index: &catalog::IndexMeta) -> bool {
    index.unique
        && index.kind == catalog::IndexKind::Scalar
        && !index.fields.is_empty()
        && index.expressions.is_empty()
        && index.predicate.is_none()
}

fn not_ready(message: String) -> QueryError {
    QueryError::Cassie(crate::app::DiagnosticError::new("55000", message).into())
}

fn unsupported_no_data() -> QueryError {
    QueryError::Cassie(
        crate::app::DiagnosticError::new(
            "42601",
            "CONCURRENTLY and WITH NO DATA options cannot be used together",
        )
        .into(),
    )
}

fn discard_new_version_output(cassie: &Cassie, name: &str, existing_versions: &HashSet<String>) {
    if let Some(version) = cassie
        .catalog
        .get_materialized_projection(name)
        .and_then(|metadata| {
            metadata
                .versions
                .into_iter()
                .find(|version| !existing_versions.contains(&version.version_id))
        })
    {
        discard_output(cassie, &version.output_collection);
    }
}

fn discard_output(cassie: &Cassie, output: &str) {
    if cassie.midge.collection_schema(output).is_some() {
        if let Err(error) = cassie.midge.drop_collection(output) {
            tracing::warn!(
                collection = output,
                error = error.to_string().as_str(),
                "failed to discard an unpublished concurrent materialized-view output"
            );
        }
    }
    let _ = cassie.catalog.unregister_collection(output);
}

fn mark_version_failed(
    cassie: &Cassie,
    name: &str,
    version_id: &str,
    message: &str,
) -> Result<(), QueryError> {
    let mut metadata = sql_materialized_view_metadata(cassie, name)?;
    if let Some(version) = metadata
        .versions
        .iter_mut()
        .find(|version| version.version_id == version_id)
    {
        version.state = catalog::ProjectionVersionState::Failed;
        version.last_error = Some(message.to_string());
    }
    metadata.last_error = Some(message.to_string());
    super::materialized_projection::persist_projection_metadata(cassie, metadata)
}
