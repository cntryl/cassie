use super::search::*;
use super::*;

pub(super) fn evaluate_function<R: RowAccess + ?Sized>(
    function: &FunctionCall,
    row: &R,
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    local_args: Option<&HashMap<String, Value>>,
) -> Result<Value, QueryError> {
    let name = function.name.to_ascii_lowercase();
    if name == "coalesce" {
        return evaluate_coalesce(
            function,
            row,
            params,
            search_context,
            user_functions,
            session,
            local_args,
        );
    }

    let args: Vec<Value> = function
        .args
        .iter()
        .map(|arg| {
            evaluate_expr_value(
                row,
                arg,
                params,
                search_context,
                user_functions,
                session,
                local_args,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    let cacheable = name != "coalesce" && has_only_constant_args(&function.args);
    let cache_key = if cacheable {
        Some(function_cache_key(&name, &args))
    } else {
        None
    };

    if cacheable {
        if let Some(cached) =
            FUNCTION_CACHE.with_borrow(|fc| fc.lookup(cache_key.unwrap()).cloned())
        {
            return Ok(cached);
        }
    }

    let result = match name.as_str() {
        "version" => {
            if !args.is_empty() {
                return Err(QueryError::General(format!("{} requires 0 args", name)));
            }
            Ok(Value::String(env!("CARGO_PKG_VERSION").to_string()))
        }
        "current_schema" => {
            if !args.is_empty() {
                return Err(QueryError::General(format!("{} requires 0 args", name)));
            }
            Ok(Value::String("public".to_string()))
        }
        "current_database" => {
            if !args.is_empty() {
                return Err(QueryError::General(format!("{} requires 0 args", name)));
            }
            Ok(Value::String(
                session
                    .and_then(|session| session.database.clone())
                    .unwrap_or_default(),
            ))
        }
        "current_user" | "session_user" | "current_role" => {
            if !args.is_empty() {
                return Err(QueryError::General(format!("{} requires 0 args", name)));
            }
            Ok(Value::String(
                session
                    .map(|session| session.user.clone())
                    .unwrap_or_else(|| "postgres".to_string()),
            ))
        }
        "length" | "len" => {
            if args.len() != 1 {
                return Err(QueryError::General(format!("{} requires 1 arg", name)));
            }
            let text = text_arg(&name, &args[0])?;
            match text {
                Some(text) => Ok(Value::Int64(text.chars().count() as i64)),
                None => Ok(Value::Null),
            }
        }
        "lower" => {
            if args.len() != 1 {
                return Err(QueryError::General(format!("{} requires 1 arg", name)));
            }
            let text = text_arg(&name, &args[0])?;
            match text {
                Some(text) => Ok(Value::String(text.to_lowercase())),
                None => Ok(Value::Null),
            }
        }
        "upper" => {
            if args.len() != 1 {
                return Err(QueryError::General(format!("{} requires 1 arg", name)));
            }
            let text = text_arg(&name, &args[0])?;
            match text {
                Some(text) => Ok(Value::String(text.to_uppercase())),
                None => Ok(Value::Null),
            }
        }
        "trim" => {
            if args.len() != 1 {
                return Err(QueryError::General(format!("{} requires 1 arg", name)));
            }
            let text = text_arg(&name, &args[0])?;
            match text {
                Some(text) => Ok(Value::String(text.trim().to_string())),
                None => Ok(Value::Null),
            }
        }
        "substring" => {
            if !(2..=3).contains(&args.len()) {
                return Err(QueryError::General(format!(
                    "{} requires 2 or 3 args, got {}",
                    name,
                    args.len()
                )));
            }
            let text = text_arg(&name, &args[0])?;
            let Some(text) = text else {
                return Ok(Value::Null);
            };
            let Some(start) = integer_arg(&name, &args[1])? else {
                return Ok(Value::Null);
            };
            let length = if args.len() == 3 {
                match integer_arg(&name, &args[2])? {
                    Some(length) => Some(length),
                    None => return Ok(Value::Null),
                }
            } else {
                None
            };
            Ok(Value::String(substring_text(&text, start, length)))
        }
        "concat" => {
            if args.is_empty() {
                return Err(QueryError::General(format!(
                    "{} requires at least 1 arg",
                    name
                )));
            }
            let mut out = String::new();
            for arg in &args {
                if !matches!(arg, Value::Null) {
                    out.push_str(&to_text(arg));
                }
            }
            Ok(Value::String(out))
        }
        "abs" => {
            if args.len() != 1 {
                return Err(QueryError::General(format!("{} requires 1 arg", name)));
            }
            match &args[0] {
                Value::Null => Ok(Value::Null),
                Value::Int64(v) => Ok(Value::Int64(v.checked_abs().unwrap_or(i64::MAX))),
                Value::Float64(v) => Ok(Value::Float64(v.abs())),
                _ => Err(QueryError::General(format!(
                    "function '{}' expects a numeric input",
                    name
                ))),
            }
        }
        "search" | "search_score" => {
            if args.len() != 2 {
                return Err(QueryError::General(format!("{} requires 2 args", name)));
            }
            let source = to_text(&args[0]);
            let query = to_text(&args[1]);
            let source_field = match &function.args[0] {
                Expr::Column(field) => Some(field.as_str()),
                _ => None,
            };
            let score = if let Some(context) = search_context {
                if source_field.is_none() && context.total_documents() > 0 {
                    context.score_text(None, &source, &query)
                } else {
                    context.score_text(source_field, &source, &query)
                }
            } else {
                simple_search_score(&source, &query)
            };
            if name.eq_ignore_ascii_case("search") {
                Ok(Value::Bool(score > 0.0))
            } else {
                Ok(Value::Float64(score))
            }
        }
        "vector_distance" => {
            let (left, right) = vector_operands(function, &args)?;
            Ok(Value::Float64(crate::vector::l2_distance(&left, &right)))
        }
        "vector_score" => {
            let (left, right) = vector_operands(function, &args)?;
            Ok(Value::Float64(
                1.0 / (1.0 + crate::vector::l2_distance(&left, &right)),
            ))
        }
        "cosine_distance" => {
            let (left, right) = vector_operands(function, &args)?;
            Ok(Value::Float64(crate::vector::cosine_distance(
                &left, &right,
            )))
        }
        "dot_product" => {
            let (left, right) = vector_operands(function, &args)?;
            Ok(Value::Float64(crate::vector::dot_score(&left, &right)))
        }
        "hybrid_score" => {
            if args.len() != 2 {
                return Err(QueryError::General(
                    "hybrid_score requires 2 args".to_string(),
                ));
            }
            Ok(Value::Float64(crate::hybrid::hybrid_score(
                scalar_to_f64(&args[0]),
                scalar_to_f64(&args[1]),
                None,
            )))
        }
        "snippet" => {
            if args.len() != 2 {
                return Err(QueryError::General(format!(
                    "snippet requires 2 args, got {}",
                    args.len()
                )));
            }
            let source = to_text(&args[0]);
            let query = to_text(&args[1]);
            let analyzer = match (&function.args[0], search_context) {
                (Expr::Column(field), Some(context)) => context.analyzer_for_field(field),
                _ => AnalyzerConfig::default(),
            };
            let terms = analyzer.analyze(&query);
            Ok(Value::String(crate::search::snippet(&source, &terms)))
        }
        "cast" => {
            if args.len() != 2 {
                return Err(QueryError::General(format!(
                    "cast requires 2 args, got {}",
                    args.len()
                )));
            }
            let target = DataType::parse_sql(&to_text(&args[1]))
                .map_err(|_| QueryError::General("cannot cast value".to_string()))?;
            let scalar = scalar_from_value(&args[0])
                .ok_or_else(|| QueryError::General("invalid cast input".to_string()))?;
            let value = cast_scalar(&scalar, &target)?;
            Ok(value.to_value())
        }
        _ => {
            let Some(metadata) = user_functions.get(&name) else {
                return Err(QueryError::General(format!(
                    "unsupported function '{name}'",
                )));
            };

            if args.len() != metadata.args.len() {
                return Err(QueryError::General(format!(
                    "function '{name}' expects {} args, got {}",
                    metadata.args.len(),
                    args.len()
                )));
            }

            let body = crate::sql::parser::parse_expression(&metadata.body).map_err(|error| {
                QueryError::General(format!("invalid function body for '{}': {}", name, error.0))
            })?;

            let locals = metadata
                .args
                .iter()
                .cloned()
                .zip(args)
                .map(|(arg, value)| (arg.name.to_ascii_lowercase(), value))
                .collect::<HashMap<String, Value>>();

            let merged_args = if let Some(outer) = local_args {
                let mut merged = outer.clone();
                for (name, value) in locals {
                    merged.insert(name, value);
                }
                merged
            } else {
                locals
            };

            eval_scalar(
                row,
                &body,
                params,
                search_context,
                user_functions,
                Some(&merged_args),
                session,
            )
            .map(|value| value.to_value())
        }
    };

    if let Some(key) = cache_key {
        if let Ok(ref value) = result {
            FUNCTION_CACHE.with_borrow_mut(|fc| fc.store(key, value.clone()));
        }
    }

    result
}

pub(super) fn evaluate_coalesce<R: RowAccess + ?Sized>(
    function: &FunctionCall,
    row: &R,
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    local_args: Option<&HashMap<String, Value>>,
) -> Result<Value, QueryError> {
    if function.args.is_empty() {
        return Err(QueryError::General(
            "coalesce requires at least 1 arg".to_string(),
        ));
    }

    for arg in &function.args {
        let value = evaluate_expr_value(
            row,
            arg,
            params,
            search_context,
            user_functions,
            session,
            local_args,
        )?;
        if !matches!(value, Value::Null) {
            return Ok(value);
        }
    }

    Ok(Value::Null)
}

pub(super) fn text_arg(name: &str, value: &Value) -> Result<Option<String>, QueryError> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(text.clone())),
        _ => Err(QueryError::General(format!(
            "function '{}' expects text input",
            name
        ))),
    }
}

pub(super) fn integer_arg(name: &str, value: &Value) -> Result<Option<usize>, QueryError> {
    match value {
        Value::Null => Ok(None),
        Value::Int64(value) if *value >= 0 => Ok(Some(*value as usize)),
        Value::Float64(value)
            if value.is_finite() && *value >= 0.0 && value.fract().abs() < f64::EPSILON =>
        {
            Ok(Some(*value as usize))
        }
        _ => Err(QueryError::General(format!(
            "function '{}' expects a non-negative integer input",
            name
        ))),
    }
}

pub(super) fn substring_text(value: &str, start: usize, length: Option<usize>) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return String::new();
    }

    let start_index = start.max(1).saturating_sub(1);
    if start_index >= chars.len() {
        return String::new();
    }

    let end_index = match length {
        Some(length) => start_index.saturating_add(length).min(chars.len()),
        None => chars.len(),
    };

    chars[start_index..end_index].iter().collect()
}

pub(super) fn scalar_to_f64(value: &Value) -> f64 {
    match value {
        Value::Float64(v) => *v,
        Value::Int64(v) => *v as f64,
        Value::Bool(v) => v.then_some(1.0).unwrap_or(0.0),
        Value::String(v) => v.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

pub(super) fn to_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Json(value) => value.to_string(),
        Value::Int64(value) => value.to_string(),
        Value::Float64(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Vector(value) => value
            .values
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(","),
        Value::Null => String::new(),
    }
}

pub(super) fn to_vector(value: &Value) -> Option<Vec<f32>> {
    match value {
        Value::Vector(vector) => Some(vector.values.clone()),
        Value::Json(json) => json
            .as_array()
            .map(|items| {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(item.as_f64()? as f32);
                }
                Some(out)
            })
            .unwrap_or(None),
        Value::String(value) => parse_vector_text(value),
        _ => None,
    }
}

pub(super) fn vector_operands(
    function: &FunctionCall,
    args: &[Value],
) -> Result<(Vec<f32>, Vec<f32>), QueryError> {
    if args.len() != 2 {
        return Err(QueryError::General(format!(
            "{} requires 2 args",
            function.name
        )));
    }

    let left = to_vector(&args[0]).ok_or_else(|| {
        QueryError::General(format!(
            "{} expects vector in first argument",
            function.name
        ))
    })?;
    let right = to_vector(&args[1]).ok_or_else(|| {
        QueryError::General(format!(
            "{} expects vector in second argument",
            function.name
        ))
    })?;

    if left.len() != right.len() {
        return Err(QueryError::General(format!(
            "{} vector length mismatch: {} != {}",
            function.name,
            left.len(),
            right.len()
        )));
    }

    Ok((left, right))
}

pub(super) fn parse_vector_text(value: &str) -> Option<Vec<f32>> {
    let trimmed = value.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return None;
    }

    let inner = trimmed.trim_start_matches('[').trim_end_matches(']');
    if inner.trim().is_empty() {
        return Some(Vec::new());
    }

    let mut out = Vec::new();
    for part in inner.split(',') {
        out.push(part.trim().parse::<f32>().ok()?);
    }
    Some(out)
}
