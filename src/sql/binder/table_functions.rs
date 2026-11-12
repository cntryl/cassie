//! Binding and schemas for FROM table functions.
use super::{
    validate_expression, BindingContext, CassieError, Catalog, DataType, Expr, FunctionCall,
    HashMap, HashSet, QuerySource, Schema,
};
fn validate_graph_table_function(
    function: &FunctionCall,
    lateral_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    if crate::sql::table_sources::is_series(&function.name) {
        if !(2..=4).contains(&function.args.len()) || function.has_modifiers() {
            return Err(crate::sql::series_signature::undefined());
        }
        for argument in &function.args {
            validate_expression(argument, lateral_fields, &HashSet::new(), false)?;
        }
        return Ok(());
    }
    if crate::sql::table_sources::is_unnest(&function.name) {
        if function.args.is_empty() || function.has_modifiers() {
            return Err(CassieError::Execution(
                "function unnest(...) does not exist".into(),
            ));
        }
        for argument in &function.args {
            if matches!(argument, Expr::Null | Expr::StringLiteral(_)) {
                return Err(CassieError::AmbiguousFunction(
                    "function unnest(unknown) is not unique".into(),
                ));
            }
            validate_expression(argument, lateral_fields, &HashSet::new(), false)?;
        }
        crate::sql::table_sources::unnest_columns(function, false, |argument| {
            super::inference::infer_expr_type(
                argument,
                &Schema { fields: Vec::new() },
                &HashMap::new(),
                &[],
            )
        })?;
        return Ok(());
    }
    if let Some(source) = crate::sql::table_sources::json_element_source(&function.name) {
        return validate_json_element_function(function, source, lateral_fields);
    }
    if crate::sql::functions::is_server_file_helper(&function.name) {
        return Err(crate::sql::functions::reject_server_file_access());
    }
    if function.name.eq_ignore_ascii_case("jsonb_path_query")
        || function.name.eq_ignore_ascii_case("jsonb_path_query_tz")
    {
        if !(2..=4).contains(&function.args.len()) || function.has_modifiers() {
            return Err(CassieError::Execution(format!(
                "function {}(jsonb, jsonpath, jsonb, boolean) does not exist",
                function.name
            )));
        }
        for arg in &function.args {
            validate_expression(arg, lateral_fields, &HashSet::new(), false)?;
        }
        return Ok(());
    }
    let modules = crate::sql::functions::is_loaded_modules(&function.name);
    let options = crate::sql::functions::is_options_table(&function.name);
    if (modules && !function.args.is_empty()) || (options && function.args.len() != 1) {
        return Err(CassieError::Execution(format!(
            "function {}(...) does not exist",
            function.name
        )));
    }
    let expected = if modules {
        0
    } else if options {
        1
    } else {
        match function.name.to_ascii_lowercase().as_str() {
            "pg_input_error_info" | "pg_catalog.pg_input_error_info" => 2,
            "pg_show_all_settings" | "pg_catalog.pg_show_all_settings" => 0,
            "graph_neighbors" => 6,
            "graph_expand" => 7,
            "graph_shortest_path" => 9,
            other => {
                return Err(CassieError::Planner(format!(
                    "unsupported table function '{other}'"
                )));
            }
        }
    };
    if function.args.len() != expected {
        return Err(CassieError::Planner(format!(
            "{} requires {} arguments",
            function.name, expected
        )));
    }
    for arg in &function.args {
        validate_expression(arg, lateral_fields, &HashSet::new(), false)?;
    }
    Ok(())
}

/// One JSON or JSONB document argument; the json and jsonb families do not cross-convert.
fn validate_json_element_function(
    function: &FunctionCall,
    source: crate::sql::table_sources::JsonElementSource,
    lateral_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    let undefined = || {
        CassieError::from(crate::app::DiagnosticError::new(
            "42883",
            format!("function {}(...) does not exist", source.function_name()),
        ))
    };
    let [argument] = function.args.as_slice() else {
        return Err(undefined());
    };
    if function.has_modifiers() {
        return Err(undefined());
    }
    validate_expression(argument, lateral_fields, &HashSet::new(), false)?;
    if matches!(argument, Expr::Null | Expr::StringLiteral(_)) {
        return Ok(());
    }
    let inferred = super::inference::infer_expr_type(
        argument,
        &Schema { fields: Vec::new() },
        &HashMap::new(),
        &[],
    );
    match inferred {
        None | Some(DataType::Null) => Ok(()),
        Some(DataType::Json) if !source.binary => Ok(()),
        Some(DataType::Jsonb) if source.binary => Ok(()),
        Some(_) => Err(undefined()),
    }
}

fn literal_string_arg(function: &FunctionCall, index: usize) -> Option<String> {
    match function.args.get(index)? {
        Expr::StringLiteral(value) => Some(value.clone()),
        _ => None,
    }
}

pub(super) fn graph_table_function_columns() -> Vec<(String, DataType)> {
    vec![
        ("depth".to_string(), DataType::BigInt),
        ("path_rank".to_string(), DataType::BigInt),
        ("cost".to_string(), DataType::Float),
        ("node_type".to_string(), DataType::Text),
        ("node_id".to_string(), DataType::Text),
        ("edge_id".to_string(), DataType::Text),
        ("edge_type".to_string(), DataType::Text),
        ("source_type".to_string(), DataType::Text),
        ("source_id".to_string(), DataType::Text),
        ("target_type".to_string(), DataType::Text),
        ("target_id".to_string(), DataType::Text),
        ("path_nodes".to_string(), DataType::Json),
        ("path_edges".to_string(), DataType::Json),
    ]
}

pub(super) fn columns(source: &QuerySource) -> Vec<(String, DataType)> {
    let QuerySource::TableFunction {
        name,
        function,
        with_ordinality,
        ..
    } = source
    else {
        return Vec::new();
    };
    if crate::sql::table_sources::is_series(&function.name) {
        let mut columns = vec![(crate::sql::table_sources::physical_name(0), DataType::Null)];
        if *with_ordinality {
            columns.push((
                crate::sql::table_sources::physical_name(1),
                DataType::BigInt,
            ));
        }
        return columns;
    }
    if crate::sql::table_sources::is_unnest(&function.name) {
        return crate::sql::table_sources::unnest_columns(function, *with_ordinality, |argument| {
            super::inference::infer_expr_type(
                argument,
                &Schema { fields: Vec::new() },
                &HashMap::new(),
                &[],
            )
        })
        .unwrap_or_default();
    }
    if let Some(json) = crate::sql::table_sources::json_element_source(&function.name) {
        return crate::sql::table_sources::json_element_columns(json, *with_ordinality);
    }
    if name.eq_ignore_ascii_case("jsonb_path_query")
        || name.eq_ignore_ascii_case("jsonb_path_query_tz")
    {
        return vec![("value".to_string(), DataType::Jsonb)];
    }
    if let Some(columns) = crate::sql::functions::catalog_table_columns(name) {
        return columns;
    }
    if matches!(
        name.to_ascii_lowercase().as_str(),
        "pg_show_all_settings" | "pg_catalog.pg_show_all_settings"
    ) {
        return vec![
            ("name".to_string(), DataType::Text),
            ("setting".to_string(), DataType::Text),
            ("unit".to_string(), DataType::Text),
            ("category".to_string(), DataType::Text),
            ("short_desc".to_string(), DataType::Text),
            ("extra_desc".to_string(), DataType::Text),
            ("context".to_string(), DataType::Text),
            ("vartype".to_string(), DataType::Text),
            ("source".to_string(), DataType::Text),
            ("min_val".to_string(), DataType::Text),
            ("max_val".to_string(), DataType::Text),
            ("enumvals".to_string(), DataType::Text),
            ("boot_val".to_string(), DataType::Text),
            ("reset_val".to_string(), DataType::Text),
            ("sourcefile".to_string(), DataType::Text),
            ("sourceline".to_string(), DataType::BigInt),
            ("pending_restart".to_string(), DataType::Boolean),
        ];
    }
    graph_table_function_columns()
}

pub(super) fn typed_columns(
    source: &QuerySource,
    outer: Option<&Schema>,
    functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Result<Vec<(String, DataType)>, CassieError> {
    if let QuerySource::TableFunction {
        function,
        with_ordinality,
        ..
    } = source
    {
        if crate::sql::table_sources::is_series(&function.name) {
            let empty = Schema { fields: Vec::new() };
            let signature = crate::sql::series_signature::resolve(function, |argument| {
                super::inference::infer_expr_type(
                    argument,
                    outer.unwrap_or(&empty),
                    functions,
                    parameter_types,
                )
            })?;
            let mut columns = vec![(
                crate::sql::table_sources::physical_name(0),
                signature.output,
            )];
            if *with_ordinality {
                columns.push((
                    crate::sql::table_sources::physical_name(1),
                    DataType::BigInt,
                ));
            }
            return Ok(columns);
        }
        if crate::sql::table_sources::is_unnest(&function.name) {
            let empty = Schema { fields: Vec::new() };
            return crate::sql::table_sources::unnest_columns(
                function,
                *with_ordinality,
                |argument| {
                    super::inference::infer_expr_type(
                        argument,
                        outer.unwrap_or(&empty),
                        functions,
                        parameter_types,
                    )
                },
            );
        }
    }
    Ok(columns(source))
}

pub(super) fn bind(
    name: String,
    mut function: FunctionCall,
    lateral: bool,
    with_ordinality: bool,
    catalog: &Catalog,
    lateral_fields: &HashSet<String>,
    context: &BindingContext,
) -> Result<QuerySource, CassieError> {
    validate_graph_table_function(&function, lateral_fields)?;
    if crate::sql::table_sources::is_generated(&function.name) {
        return Ok(QuerySource::TableFunction {
            name,
            function,
            lateral,
            with_ordinality,
        });
    }
    if crate::sql::functions::is_options_table(&function.name) {
        bind_options_argument(&mut function.args[0])?;
        let modifier = if function.is_distinct() {
            Some("DISTINCT")
        } else if !function.order_by.is_empty() {
            Some("ORDER BY")
        } else if function.filter.is_some() {
            Some("FILTER")
        } else {
            None
        };
        if let Some(modifier) = modifier {
            return Err(CassieError::Execution(format!(
                "{modifier} specified, but {} is not an aggregate function",
                function.name
            )));
        }
    }
    if let Some(graph_name) = (!crate::sql::functions::is_catalog_table(&function.name))
        .then(|| literal_string_arg(&function, 0))
        .flatten()
    {
        let graph_name = super::normalize_relation_name(&graph_name, context)?;
        if !catalog.graph_exists_exact(&graph_name) {
            return Err(CassieError::Planner(format!(
                "graph '{graph_name}' does not exist"
            )));
        }
    }
    Ok(QuerySource::TableFunction {
        name,
        function,
        lateral,
        with_ordinality,
    })
}

fn bind_options_argument(arg: &mut Expr) -> Result<(), CassieError> {
    if matches!(arg, Expr::Param(_))
        || matches!(arg,Expr::Cast{expr,data_type:DataType::Array(_),..} if matches!(expr.as_ref(),Expr::Param(_)))
    {
        return Err(CassieError::Unsupported(
            "native TEXT[] parameter identity remains pending under #793".into(),
        ));
    }
    if matches!(arg, Expr::StringLiteral(_) | Expr::Null) {
        let original = std::mem::replace(arg, Expr::Null);
        *arg = Expr::Cast {
            expr: Box::new(original),
            data_type: DataType::Array(Box::new(DataType::Text)),
            source_type: None,
            identity: crate::sql::ast::CastIdentity::Scalar,
        };
        return Ok(());
    }
    let inferred = super::inference::infer_expr_type(
        arg,
        &Schema { fields: Vec::new() },
        &HashMap::new(),
        &[],
    );
    if inferred.is_some_and(|data_type| {
        data_type != DataType::Null && data_type != DataType::Array(Box::new(DataType::Text))
    }) {
        return Err(CassieError::Execution(
            "function pg_options_to_table(...) does not exist".into(),
        ));
    }
    Ok(())
}
