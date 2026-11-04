//! Existing catalog/graph table-function dispatch and selected generated sources.
use super::{
    batch, filter, finalize_source_batches, graph, jsonpath_table, source_options, BatchRow,
    QueryError, QuerySource, SourceExecution, SourceExecutionEnv, Value,
};

pub(super) fn execute(
    env: &SourceExecutionEnv<'_>,
    source: &QuerySource,
    outer_row: Option<&BatchRow>,
    qualify: bool,
    row_budget: Option<usize>,
    ctes: &super::CteContext,
) -> SourceExecution {
    let QuerySource::TableFunction {
        name,
        function,
        lateral,
        with_ordinality,
    } = source
    else {
        unreachable!("table function dispatch requires a table function");
    };
    let lateral = *lateral;
    if crate::sql::table_sources::is_series(&function.name) {
        return super::source_series::execute(
            env,
            function,
            *with_ordinality,
            outer_row,
            row_budget,
            ctes,
        );
    }
    if crate::sql::table_sources::is_unnest(&function.name) {
        return super::source_set_function::execute(
            env,
            function,
            *with_ordinality,
            outer_row,
            row_budget,
            ctes,
        );
    }
    if crate::sql::functions::is_options_table(&function.name) {
        let batches = source_options::execute(env, &function.args[0], outer_row)?;
        return finalize_source_batches(env, batches, Vec::new(), qualify, name);
    }
    if crate::sql::functions::is_loaded_modules(&function.name) {
        env.controls.check_interruption()?;
        // Native plugin loading is absent from the current Cassie profile.
        return finalize_source_batches(env, Vec::new(), Vec::new(), qualify, name);
    }
    if crate::sql::functions::is_pg_input_error_info(&function.name) {
        return execute_pg_input_error_info(
            env,
            name,
            function,
            if lateral { outer_row } else { None },
            qualify,
        );
    }
    if matches!(
        function.name.to_ascii_lowercase().as_str(),
        "pg_show_all_settings" | "pg_catalog.pg_show_all_settings"
    ) {
        return execute_settings(env, name, qualify);
    }
    let (rows, _table_memory) = if function.name.eq_ignore_ascii_case("jsonb_path_query")
        || function.name.eq_ignore_ascii_case("jsonb_path_query_tz")
    {
        jsonpath_table::execute(env, function, if lateral { outer_row } else { None })?
    } else {
        graph::execute_table_function(env, function, if lateral { outer_row } else { None })?
            .into_parts()
    };
    finalize_source_batches(
        env,
        batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE),
        Vec::new(),
        qualify,
        name,
    )
}

fn execute_settings(env: &SourceExecutionEnv<'_>, name: &str, qualify: bool) -> SourceExecution {
    let session = env.session.ok_or_else(|| {
        QueryError::General("pg_show_all_settings requires a session".to_string())
    })?;
    let rows = crate::app::all_session_settings()
        .iter()
        .map(|spec| {
            BatchRow::new(vec![
                ("name".to_string(), Value::String(spec.name.to_string())),
                (
                    "setting".to_string(),
                    Value::String(
                        session
                            .setting(spec.name)
                            .unwrap_or_else(|_| spec.default.to_string()),
                    ),
                ),
                ("unit".to_string(), Value::Null),
                (
                    "category".to_string(),
                    Value::String("Client Connection Defaults".to_string()),
                ),
                (
                    "short_desc".to_string(),
                    Value::String("Cassie compatibility setting".to_string()),
                ),
                ("extra_desc".to_string(), Value::Null),
                (
                    "context".to_string(),
                    Value::String(
                        if spec.kind == crate::app::SettingKind::Mutable {
                            "user"
                        } else {
                            "internal"
                        }
                        .to_string(),
                    ),
                ),
                ("vartype".to_string(), Value::String("string".to_string())),
                ("source".to_string(), Value::String("default".to_string())),
                ("min_val".to_string(), Value::Null),
                ("max_val".to_string(), Value::Null),
                ("enumvals".to_string(), Value::Null),
                (
                    "boot_val".to_string(),
                    Value::String(spec.default.to_string()),
                ),
                (
                    "reset_val".to_string(),
                    Value::String(spec.default.to_string()),
                ),
                ("sourcefile".to_string(), Value::Null),
                ("sourceline".to_string(), Value::Null),
                ("pending_restart".to_string(), Value::Bool(false)),
            ])
        })
        .collect::<Vec<_>>();
    finalize_source_batches(
        env,
        batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE),
        Vec::new(),
        qualify,
        name,
    )
}

fn execute_pg_input_error_info(
    env: &SourceExecutionEnv<'_>,
    name: &str,
    function: &crate::sql::ast::FunctionCall,
    outer_row: Option<&BatchRow>,
    qualify: bool,
) -> SourceExecution {
    let args = graph::evaluate_args(env, function, outer_row)?;
    let input = args
        .first()
        .ok_or_else(|| QueryError::General("pg_input_error_info requires 2 arguments".into()))?;
    let type_name = args
        .get(1)
        .ok_or_else(|| QueryError::General("pg_input_error_info requires 2 arguments".into()))?;
    let (Value::String(input), Value::String(type_name)) = (input, type_name) else {
        return Err(QueryError::General(
            "pg_input_error_info arguments must be text".into(),
        ));
    };
    let data_type = crate::types::DataType::parse_sql(type_name).map_err(QueryError::General)?;
    let result = filter::character_arrays::validate_input(input, &data_type, Some(env.controls))
        .unwrap_or_else(|| {
            filter::cast_value(&Value::String(input.clone()), &data_type).map(|_| ())
        });
    if let Err(
        error @ QueryError::Cassie(
            crate::app::CassieError::ResourceLimit(_)
            | crate::app::CassieError::QueryCancelled
            | crate::app::CassieError::DeadlineExceeded,
        ),
    ) = result
    {
        return Err(error);
    }
    let rows = match result {
        Ok(()) => vec![BatchRow::new(vec![
            ("message".into(), Value::Null),
            ("detail".into(), Value::Null),
            ("hint".into(), Value::Null),
            ("sql_error_code".into(), Value::Null),
        ])],
        Err(error) => {
            let message = error.to_string();
            let cassie_error = crate::app::CassieError::from(error);
            let code = cassie_error.descriptor().sql_state;
            vec![BatchRow::new(vec![
                ("message".into(), Value::String(message)),
                ("detail".into(), Value::Null),
                ("hint".into(), Value::Null),
                ("sql_error_code".into(), Value::String(code.to_owned())),
            ])]
        }
    };
    finalize_source_batches(
        env,
        batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE),
        Vec::new(),
        qualify,
        name,
    )
}
