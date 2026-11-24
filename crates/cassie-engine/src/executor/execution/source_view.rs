use super::super::{
    build_logical_plan_in_context, build_logical_plan_in_session,
    execute_plan_with_parameter_oids, plan_execution_env_with_binding_context,
};
use super::{
    batch, finalize_source_batches, schema_text_fields, source_rows::project_rows_to_schema,
    CteContext, QueryError, SourceExecution, SourceExecutionEnv,
};

pub(super) fn execute_view_source(
    env: &SourceExecutionEnv<'_>,
    name: &str,
    view: &crate::catalog::ViewMeta,
    qualify: bool,
) -> SourceExecution {
    let parsed = env
        .cassie
        .parse_statement_for_session(env.session, &view.query)
        .map_err(|error| QueryError::General(error.to_string()))?;
    crate::sql::definition_guard::view(view).map_err(QueryError::Cassie)?;
    let stored_context = stored_view_context(env, &parsed, view);
    let (logical, stored_functions) = if let Some(context) = &stored_context {
        let all_functions = env.cassie.catalog.list_functions();
        let mut functions = crate::catalog::function_resolution::functions_for_scope(
            &all_functions,
            &context.database,
            &context.search_path,
            context.scopes_database_objects(),
        );
        let mut calls = Vec::new();
        if let crate::sql::ast::QueryStatement::Select(select) = &parsed.statement {
            crate::sql::binder::collect_functions(select, &mut calls);
        }
        for call in calls {
            let selected = all_functions.iter().find(|candidate| {
                view.function_dependency_ids.contains(&candidate.object_id)
                    && crate::catalog::local_name(&candidate.name)
                        .eq_ignore_ascii_case(&crate::catalog::local_name(&call.name))
            });
            if let Some(selected) = selected {
                functions.insert(call.name.to_ascii_lowercase(), selected.clone());
            }
        }
        (
            build_logical_plan_in_context(env.cassie, context, &parsed)?,
            Some(functions),
        )
    } else {
        (
            build_logical_plan_in_session(env.cassie, env.session, &parsed)?,
            None,
        )
    };
    let mut view_cte_context = CteContext::new();
    let rows = if let Some(functions) = &stored_functions {
        let context = stored_context.expect("stored view function scope");
        let plan_env = plan_execution_env_with_binding_context(
            env.cassie,
            env.session,
            &context,
            functions,
            env.params,
            env.parameter_type_oids,
            env.controls,
        );
        execute_plan_with_parameter_oids(&plan_env, &logical, &mut view_cte_context)?
    } else {
        execute_plan_with_parameter_oids(&env.plan_env(), &logical, &mut view_cte_context)?
    };
    let rows = project_rows_to_schema(rows, &view.schema, name)?;
    let text_fields = schema_text_fields(&view.schema);
    finalize_source_batches(
        env,
        batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE),
        text_fields,
        qualify,
        name,
    )
}

fn stored_view_context(
    env: &SourceExecutionEnv<'_>,
    parsed: &crate::sql::ast::ParsedStatement,
    view: &crate::catalog::ViewMeta,
) -> Option<crate::sql::binder::BindingContext> {
    if view.function_dependency_database.is_empty()
        && view.function_dependency_search_path.is_empty()
    {
        return None;
    }
    let database = if view.function_dependency_database.is_empty() {
        env.cassie.default_database.clone()
    } else {
        view.function_dependency_database.clone()
    };
    let raw_catalog_names = match &parsed.statement {
        crate::sql::ast::QueryStatement::Select(select) => {
            crate::sql::binder::select_referenced_relations(select)
                .iter()
                .any(|name| env.cassie.catalog.get_collection(name).is_some())
        }
        _ => false,
    };
    let search_schemas = view
        .function_dependency_search_path
        .iter()
        .map(|schema| schema.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    let legacy_function_names = env.cassie.catalog.list_functions().iter().any(|function| {
        matches!(
            crate::catalog::parse_name(&function.name),
            Ok(crate::catalog::ParsedName::SchemaQualified { schema, .. })
                if search_schemas.contains(&schema.to_ascii_lowercase())
        )
    });
    let legacy_graph_names = env
        .cassie
        .catalog
        .list_property_graphs()
        .iter()
        .any(|graph| {
            matches!(
                crate::catalog::parse_name(&graph.name),
                Ok(crate::catalog::ParsedName::SchemaQualified { .. }
                    | crate::catalog::ParsedName::Unqualified(_))
            )
        });
    let retained_function_dependencies = env
        .cassie
        .catalog
        .list_functions()
        .iter()
        .any(|function| view.function_dependency_ids.contains(&function.object_id));
    let unscoped = env.cassie.catalog.list_namespaces().is_empty()
        || raw_catalog_names
        || legacy_function_names
        || legacy_graph_names
        || retained_function_dependencies;
    Some(if unscoped {
        crate::sql::binder::BindingContext::unscoped(
            database,
            view.function_dependency_search_path.clone(),
        )
    } else {
        crate::sql::binder::BindingContext::scoped(
            database,
            view.function_dependency_search_path.clone(),
        )
    })
}
