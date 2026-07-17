use super::search::{simple_search_score, SearchContext};
use super::{
    cast_scalar, eval_scalar, evaluate_expr_value, scalar_from_value, AnalyzerConfig,
    CassieSession, DataType, EvalContext, Expr, FunctionCall, FunctionMeta, HashMap, QueryError,
    Value,
};
use crate::catalog::{name_matches, DEFAULT_SCHEMA, PG_CATALOG_SCHEMA};
use crate::executor::batch::RowAccess;
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, PrimitiveDateTime, UtcOffset};

pub(super) fn evaluate_function<R: RowAccess + ?Sized>(
    function: &FunctionCall,
    row: &R,
    context: EvalContext<'_>,
) -> Result<Value, QueryError> {
    let name = function.name.to_ascii_lowercase();
    if name == "coalesce" {
        return evaluate_coalesce(
            function,
            row,
            context.params,
            context.search_context,
            context.user_functions,
            context.session,
            context.local_args,
        );
    }

    let args: Vec<Value> = function
        .args
        .iter()
        .map(|arg| {
            evaluate_expr_value(
                row,
                arg,
                context.params,
                context.search_context,
                context.user_functions,
                context.session,
                context.local_args,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    evaluate_builtin_function(
        &name,
        function,
        row,
        &args,
        context.search_context,
        context.session,
    )
    .unwrap_or_else(|| evaluate_user_defined_function(&name, row, context, args))
}

fn evaluate_builtin_function<R: RowAccess + ?Sized>(
    name: &str,
    function: &FunctionCall,
    row: &R,
    args: &[Value],
    search_context: Option<&SearchContext>,
    session: Option<&CassieSession>,
) -> Option<Result<Value, QueryError>> {
    evaluate_system_function(name, row, args, session)
        .or_else(|| evaluate_text_function(name, args))
        .or_else(|| evaluate_search_function(name, function, args, search_context))
        .or_else(|| evaluate_vector_function(name, function, args))
        .or_else(|| evaluate_cast_function(name, args))
        .or_else(|| (name == "time_bucket").then(|| evaluate_time_bucket(args)))
}

fn evaluate_system_function<R: RowAccess + ?Sized>(
    name: &str,
    row: &R,
    args: &[Value],
    session: Option<&CassieSession>,
) -> Option<Result<Value, QueryError>> {
    match name {
        "version" => Some(
            require_zero_args(name, args)
                .map(|()| Value::String(env!("CARGO_PKG_VERSION").to_string())),
        ),
        "pg_catalog.version" => Some(require_zero_args(name, args).map(|()| {
            Value::String(format!(
                "PostgreSQL 16.0 compatible Cassie {}",
                env!("CARGO_PKG_VERSION")
            ))
        })),
        "current_schema" => Some(require_zero_args(name, args).map(|()| {
            Value::String(
                session.map_or_else(|| "public".to_string(), CassieSession::current_schema),
            )
        })),
        "current_database" => Some(require_zero_args(name, args).map(|()| {
            Value::String(
                session
                    .and_then(|session| session.database.clone())
                    .unwrap_or_default(),
            )
        })),
        "current_user" | "session_user" | "current_role" => {
            Some(require_zero_args(name, args).map(|()| {
                Value::String(
                    session.map_or_else(|| "postgres".to_string(), |session| session.user.clone()),
                )
            }))
        }
        "quote_ident" | "pg_catalog.quote_ident" => Some(unary_nullable_text(name, args, |text| {
            Value::String(quote_identifier(text))
        })),
        "format_type" | "pg_catalog.format_type" => Some(evaluate_format_type(name, args)),
        "pg_get_expr" | "pg_catalog.pg_get_expr" => Some(unary_nullable_text(name, args, |text| {
            Value::String(text.to_string())
        })),
        "pg_get_userbyid" | "pg_catalog.pg_get_userbyid" => {
            Some(require_arg_count(name, args, 1).map(|()| Value::String("postgres".to_string())))
        }
        "obj_description" | "pg_catalog.obj_description" => {
            Some(require_arg_count_range(name, args, 1..=2).map(|()| Value::Null))
        }
        "has_schema_privilege"
        | "pg_catalog.has_schema_privilege"
        | "has_table_privilege"
        | "pg_catalog.has_table_privilege" => {
            Some(require_arg_count_range(name, args, 2..=3).map(|()| Value::Bool(true)))
        }
        "pg_table_is_visible" | "pg_catalog.pg_table_is_visible" => Some(
            require_arg_count(name, args, 1)
                .map(|()| Value::Bool(pg_table_is_visible(row, args, session))),
        ),
        _ => None,
    }
}

fn pg_table_is_visible<R: RowAccess + ?Sized>(
    row: &R,
    args: &[Value],
    session: Option<&CassieSession>,
) -> bool {
    if let Some(Value::Int64(expected_oid)) = args.first() {
        let Some(actual_oid) = row.get("oid").and_then(value_as_i64) else {
            return false;
        };
        if actual_oid != *expected_oid {
            return false;
        }
    }

    let schema = row
        .get("relnamespace")
        .and_then(value_as_string)
        .or_else(|| row.get("schemaname").and_then(value_as_string))
        .or_else(|| row.get("table_schema").and_then(value_as_string))
        .unwrap_or_else(|| DEFAULT_SCHEMA.to_string());
    if schema.eq_ignore_ascii_case(PG_CATALOG_SCHEMA) {
        return true;
    }

    session.is_none_or(|session| {
        session
            .search_path()
            .into_iter()
            .any(|entry| entry.eq_ignore_ascii_case(&schema))
    })
}

fn value_as_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Int64(value) => Some(*value),
        _ => None,
    }
}

fn value_as_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        _ => None,
    }
}

fn evaluate_text_function(name: &str, args: &[Value]) -> Option<Result<Value, QueryError>> {
    match name {
        "length" | "len" => Some(unary_nullable_text(name, args, |text| {
            Value::Int64(i64::try_from(text.chars().count()).unwrap_or(i64::MAX))
        })),
        "lower" => Some(unary_nullable_text(name, args, |text| {
            Value::String(text.to_lowercase())
        })),
        "upper" => Some(unary_nullable_text(name, args, |text| {
            Value::String(text.to_uppercase())
        })),
        "trim" => Some(unary_nullable_text(name, args, |text| {
            Value::String(text.trim().to_string())
        })),
        "substring" => Some(evaluate_substring(name, args)),
        "concat" => Some(evaluate_concat(name, args)),
        "abs" => Some(evaluate_abs(name, args)),
        _ => None,
    }
}

fn evaluate_search_function(
    name: &str,
    function: &FunctionCall,
    args: &[Value],
    search_context: Option<&SearchContext>,
) -> Option<Result<Value, QueryError>> {
    match name {
        "search" | "search_score" => {
            Some(evaluate_search_score(name, function, args, search_context))
        }
        "snippet" => Some(evaluate_snippet(function, args, search_context)),
        _ => None,
    }
}

fn evaluate_vector_function(
    name: &str,
    function: &FunctionCall,
    args: &[Value],
) -> Option<Result<Value, QueryError>> {
    match name {
        "vector_distance" => Some(
            vector_operands(function, args)
                .map(|(left, right)| Value::Float64(crate::vector::l2_distance(&left, &right))),
        ),
        "vector_score" => Some(vector_operands(function, args).map(|(left, right)| {
            Value::Float64(1.0 / (1.0 + crate::vector::l2_distance(&left, &right)))
        })),
        "cosine_distance" => Some(
            vector_operands(function, args)
                .map(|(left, right)| Value::Float64(crate::vector::cosine_distance(&left, &right))),
        ),
        "dot_product" => Some(
            vector_operands(function, args)
                .map(|(left, right)| Value::Float64(crate::vector::dot_score(&left, &right))),
        ),
        "hybrid_score" => Some(evaluate_hybrid_score(args)),
        _ => None,
    }
}

fn evaluate_cast_function(name: &str, args: &[Value]) -> Option<Result<Value, QueryError>> {
    (name == "cast").then(|| {
        require_arg_count("cast", args, 2)?;
        let target = DataType::parse_sql(&to_text(&args[1]))
            .map_err(|_| QueryError::General("cannot cast value".to_string()))?;
        let value = cast_scalar(&scalar_from_value(&args[0]), &target)?;
        Ok(value.to_value())
    })
}

fn evaluate_user_defined_function<R: RowAccess + ?Sized>(
    name: &str,
    row: &R,
    context: EvalContext<'_>,
    args: Vec<Value>,
) -> Result<Value, QueryError> {
    let lookup = name.to_ascii_lowercase();
    let Some(metadata) = context
        .user_functions
        .get(name)
        .or_else(|| context.user_functions.get(&lookup))
        .or_else(|| {
            context
                .user_functions
                .values()
                .find(|metadata| name_matches(&metadata.name, name))
        })
    else {
        return Err(QueryError::General(format!(
            "unsupported function '{name}'"
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
        QueryError::General(format!("invalid function body for '{name}': {error}"))
    })?;
    let locals = metadata
        .args
        .iter()
        .zip(args)
        .map(|(arg, value)| {
            let value = cast_scalar(&scalar_from_value(&value), &arg.data_type)?;
            Ok((arg.name.to_ascii_lowercase(), value.to_value()))
        })
        .collect::<Result<HashMap<String, Value>, QueryError>>()?;
    let merged_args = merge_local_args(context.local_args, locals);
    eval_scalar(
        row,
        &body,
        context.params,
        context.search_context,
        context.user_functions,
        Some(&merged_args),
        context.session,
    )
    .and_then(|value| cast_scalar(&value, &metadata.return_type))
    .map(|value| value.to_value())
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
            "function '{name}' expects text input"
        ))),
    }
}

pub(super) fn integer_arg(name: &str, value: &Value) -> Result<Option<usize>, QueryError> {
    match value {
        Value::Null => Ok(None),
        Value::Int64(value) if *value >= 0 => Ok(usize::try_from(*value).ok()),
        Value::Float64(value)
            if value.is_finite() && *value >= 0.0 && value.fract().abs() < f64::EPSILON =>
        {
            Ok(parse_f64_to_usize(*value))
        }
        _ => Err(QueryError::General(format!(
            "function '{name}' expects a non-negative integer input"
        ))),
    }
}

fn signed_integer_arg(name: &str, value: &Value) -> Result<Option<i64>, QueryError> {
    match value {
        Value::Null => Ok(None),
        Value::Int64(value) => Ok(Some(*value)),
        Value::Float64(value) if value.is_finite() && value.fract().abs() < f64::EPSILON => {
            Ok(parse_f64_to_i64(*value))
        }
        _ => Err(QueryError::General(format!(
            "function '{name}' expects an integer input"
        ))),
    }
}

fn quote_identifier(value: &str) -> String {
    let simple = value
        .chars()
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_ascii_lowercase())
        && value
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_lowercase() || ch.is_ascii_digit());
    if simple && !is_reserved_identifier(value) {
        return value.to_string();
    }
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn is_reserved_identifier(value: &str) -> bool {
    matches!(
        value,
        "select" | "from" | "where" | "table" | "view" | "index" | "user" | "role" | "schema"
    )
}

fn format_type_oid(oid: i64, typmod: i64) -> String {
    match oid {
        16 => "boolean".to_string(),
        17 => "bytea".to_string(),
        20 => "bigint".to_string(),
        21 => "smallint".to_string(),
        23 => "integer".to_string(),
        25 => "text".to_string(),
        114 => "json".to_string(),
        701 => "double precision".to_string(),
        1042 => format_character_type("character", typmod),
        1043 => format_character_type("character varying", typmod),
        1082 => "date".to_string(),
        1083 => "time without time zone".to_string(),
        1114 => "timestamp without time zone".to_string(),
        2950 => "uuid".to_string(),
        oid if (33_000..34_000).contains(&oid) => {
            format!("vector({})", oid.saturating_sub(33_000))
        }
        oid if (34_000..50_000).contains(&oid) => "array".to_string(),
        _ => oid.to_string(),
    }
}

fn format_character_type(name: &str, typmod: i64) -> String {
    if typmod >= 4 {
        format!("{name}({})", typmod - 4)
    } else {
        name.to_string()
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
        Value::Int64(v) => parse_i64_to_f64(*v).unwrap_or(0.0),
        Value::Bool(v) => v.then_some(1.0).unwrap_or(0.0),
        Value::String(v) => v.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn evaluate_time_bucket(args: &[Value]) -> Result<Value, QueryError> {
    if !(2..=3).contains(&args.len()) {
        return Err(QueryError::General(format!(
            "time_bucket requires 2 or 3 args, got {}",
            args.len()
        )));
    }
    if args.iter().any(|arg| matches!(arg, Value::Null)) {
        return Ok(Value::Null);
    }

    let width_ns = duration_width_ns(&args[0])?;
    let timestamp_ns = timestamp_arg_ns("time_bucket", &args[1])?;
    let origin_ns = if args.len() == 3 {
        timestamp_arg_ns("time_bucket", &args[2])?
    } else {
        0
    };
    let delta = timestamp_ns
        .checked_sub(origin_ns)
        .ok_or_else(|| QueryError::General("time_bucket timestamp overflow".to_string()))?;
    let bucket_index = floor_div(delta, width_ns);
    let bucket_offset = bucket_index
        .checked_mul(width_ns)
        .ok_or_else(|| QueryError::General("time_bucket timestamp overflow".to_string()))?;
    let bucket_ns = origin_ns
        .checked_add(bucket_offset)
        .ok_or_else(|| QueryError::General("time_bucket timestamp overflow".to_string()))?;
    let bucket = OffsetDateTime::from_unix_timestamp_nanos(bucket_ns)
        .map_err(|_| QueryError::General("time_bucket timestamp overflow".to_string()))?
        .to_offset(UtcOffset::UTC);
    let formatted = bucket
        .format(&Rfc3339)
        .map_err(|error| QueryError::General(error.to_string()))?;
    Ok(Value::String(formatted))
}

fn duration_width_ns(value: &Value) -> Result<i128, QueryError> {
    let Value::String(raw) = value else {
        return Err(QueryError::General(
            "time_bucket width must be a duration string".to_string(),
        ));
    };
    let (amount, unit) = parse_duration_parts(raw)?;
    let multiplier = match unit.as_str() {
        "ns" | "nanosecond" | "nanoseconds" => 1,
        "us" | "microsecond" | "microseconds" => 1_000,
        "ms" | "millisecond" | "milliseconds" => 1_000_000,
        "s" | "sec" | "second" | "seconds" => 1_000_000_000,
        "m" | "min" | "minute" | "minutes" => 60_000_000_000,
        "h" | "hr" | "hour" | "hours" => 3_600_000_000_000,
        "d" | "day" | "days" => 86_400_000_000_000,
        "w" | "week" | "weeks" => 604_800_000_000_000,
        "month" | "months" | "year" | "years" => {
            return Err(QueryError::General(
                "time_bucket width cannot use calendar-dependent units".to_string(),
            ));
        }
        _ => {
            return Err(QueryError::General(format!(
                "invalid time_bucket width unit '{unit}'"
            )));
        }
    };
    amount
        .checked_mul(multiplier)
        .filter(|width| *width > 0)
        .ok_or_else(|| QueryError::General("time_bucket width overflow".to_string()))
}

fn parse_duration_parts(raw: &str) -> Result<(i128, String), QueryError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(QueryError::General("invalid time_bucket width".to_string()));
    }
    let split_at = value
        .find(|ch: char| !(ch.is_ascii_digit() || ch == '-' || ch == '+'))
        .unwrap_or(value.len());
    let number = value[..split_at].trim();
    let unit = value[split_at..].trim().to_ascii_lowercase();
    if number.is_empty() || unit.is_empty() {
        return Err(QueryError::General("invalid time_bucket width".to_string()));
    }
    let amount = number
        .parse::<i128>()
        .map_err(|_| QueryError::General("invalid time_bucket width amount".to_string()))?;
    if amount <= 0 {
        return Err(QueryError::General(
            "time_bucket width must be positive".to_string(),
        ));
    }
    Ok((amount, unit))
}

fn timestamp_arg_ns(name: &str, value: &Value) -> Result<i128, QueryError> {
    let Value::String(raw) = value else {
        return Err(QueryError::General(format!(
            "function '{name}' expects timestamp input"
        )));
    };
    parse_timestamp_utc(raw)
}

fn parse_timestamp_utc(raw: &str) -> Result<i128, QueryError> {
    if let Ok(value) = OffsetDateTime::parse(raw, &Rfc3339) {
        return Ok(value.to_offset(UtcOffset::UTC).unix_timestamp_nanos());
    }

    let normalized = raw.trim().replace(' ', "T");
    for format in [
        "[year]-[month]-[day]T[hour]:[minute]:[second]",
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond]",
    ] {
        let description = time::format_description::parse_owned::<2>(format)
            .map_err(|error| QueryError::General(error.to_string()))?;
        if let Ok(value) = PrimitiveDateTime::parse(&normalized, &description) {
            return Ok(value.assume_utc().unix_timestamp_nanos());
        }
    }

    Err(QueryError::General(
        "invalid time_bucket timestamp".to_string(),
    ))
}

fn floor_div(value: i128, divisor: i128) -> i128 {
    let quotient = value / divisor;
    let remainder = value % divisor;
    if remainder != 0 && ((remainder > 0) != (divisor > 0)) {
        quotient - 1
    } else {
        quotient
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
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
            .join(","),
        Value::Null => String::new(),
    }
}

pub(super) fn to_vector(value: &Value) -> Option<Vec<f32>> {
    match value {
        Value::Vector(vector) => Some(vector.values.clone()),
        Value::Json(json) => json.as_array().and_then(|items| {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(parse_f64_to_f32(item.as_f64()?)?);
            }
            Some(out)
        }),
        Value::String(value) => parse_vector_text(value),
        _ => None,
    }
}

fn require_zero_args(name: &str, args: &[Value]) -> Result<(), QueryError> {
    require_arg_count(name, args, 0)
}

fn require_arg_count(name: &str, args: &[Value], expected: usize) -> Result<(), QueryError> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(QueryError::General(format!(
            "{name} requires {expected} arg{}",
            if expected == 1 { "" } else { "s" }
        )))
    }
}

fn require_arg_count_range(
    name: &str,
    args: &[Value],
    range: std::ops::RangeInclusive<usize>,
) -> Result<(), QueryError> {
    if range.contains(&args.len()) {
        Ok(())
    } else {
        Err(QueryError::General(format!(
            "{name} requires {} or {} args",
            range.start(),
            range.end()
        )))
    }
}

fn unary_nullable_text(
    name: &str,
    args: &[Value],
    map: impl FnOnce(&str) -> Value,
) -> Result<Value, QueryError> {
    require_arg_count(name, args, 1)?;
    Ok(text_arg(name, &args[0])?.map_or(Value::Null, |text| map(&text)))
}

fn evaluate_format_type(name: &str, args: &[Value]) -> Result<Value, QueryError> {
    require_arg_count(name, args, 2)?;
    let oid = signed_integer_arg(name, &args[0])?;
    let typmod = signed_integer_arg(name, &args[1])?;
    Ok(match (oid, typmod) {
        (Some(oid), Some(typmod)) => Value::String(format_type_oid(oid, typmod)),
        _ => Value::Null,
    })
}

fn evaluate_substring(name: &str, args: &[Value]) -> Result<Value, QueryError> {
    if !(2..=3).contains(&args.len()) {
        return Err(QueryError::General(format!(
            "{name} requires 2 or 3 args, got {}",
            args.len()
        )));
    }
    let Some(text) = text_arg(name, &args[0])? else {
        return Ok(Value::Null);
    };
    let Some(start) = integer_arg(name, &args[1])? else {
        return Ok(Value::Null);
    };
    let length = if args.len() == 3 {
        match integer_arg(name, &args[2])? {
            Some(length) => Some(length),
            None => return Ok(Value::Null),
        }
    } else {
        None
    };
    Ok(Value::String(substring_text(&text, start, length)))
}

fn evaluate_concat(name: &str, args: &[Value]) -> Result<Value, QueryError> {
    if args.is_empty() {
        return Err(QueryError::General(format!(
            "{name} requires at least 1 arg"
        )));
    }
    let mut out = String::new();
    for arg in args {
        if !matches!(arg, Value::Null) {
            out.push_str(&to_text(arg));
        }
    }
    Ok(Value::String(out))
}

fn evaluate_abs(name: &str, args: &[Value]) -> Result<Value, QueryError> {
    require_arg_count(name, args, 1)?;
    match &args[0] {
        Value::Null => Ok(Value::Null),
        Value::Int64(v) => Ok(Value::Int64(v.checked_abs().unwrap_or(i64::MAX))),
        Value::Float64(v) => Ok(Value::Float64(v.abs())),
        _ => Err(QueryError::General(format!(
            "function '{name}' expects a numeric input"
        ))),
    }
}

fn evaluate_search_score(
    name: &str,
    function: &FunctionCall,
    args: &[Value],
    search_context: Option<&SearchContext>,
) -> Result<Value, QueryError> {
    require_arg_count(name, args, 2)?;
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

fn evaluate_snippet(
    function: &FunctionCall,
    args: &[Value],
    search_context: Option<&SearchContext>,
) -> Result<Value, QueryError> {
    require_arg_count("snippet", args, 2)?;
    let source = to_text(&args[0]);
    let query = to_text(&args[1]);
    let analyzer = match (&function.args[0], search_context) {
        (Expr::Column(field), Some(context)) => context.analyzer_for_field(field),
        _ => AnalyzerConfig::default(),
    };
    let terms = analyzer.analyze(&query);
    Ok(Value::String(crate::search::snippet(&source, &terms)))
}

fn evaluate_hybrid_score(args: &[Value]) -> Result<Value, QueryError> {
    require_arg_count("hybrid_score", args, 2)?;
    Ok(Value::Float64(crate::hybrid::hybrid_score(
        scalar_to_f64(&args[0]),
        scalar_to_f64(&args[1]),
        None,
    )))
}

fn merge_local_args(
    outer: Option<&HashMap<String, Value>>,
    locals: HashMap<String, Value>,
) -> HashMap<String, Value> {
    if let Some(outer) = outer {
        let mut merged = outer.clone();
        for (name, value) in locals {
            merged.insert(name, value);
        }
        merged
    } else {
        locals
    }
}

fn parse_i64_to_f64(value: i64) -> Option<f64> {
    value.to_string().parse::<f64>().ok()
}

fn parse_f64_to_i64(value: f64) -> Option<i64> {
    if !value.is_finite() || value.fract() != 0.0 {
        return None;
    }
    format!("{value:.0}").parse::<i64>().ok()
}

fn parse_f64_to_usize(value: f64) -> Option<usize> {
    if !value.is_finite() || value.fract() != 0.0 || value < 0.0 {
        return None;
    }
    format!("{value:.0}").parse::<usize>().ok()
}

fn parse_f64_to_f32(value: f64) -> Option<f32> {
    value.to_string().parse::<f32>().ok()
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
