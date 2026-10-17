//! PostgreSQL states for execution and expression diagnostics.
use super::{
    bad_request_descriptor, cancellation_descriptor, pagination_sql_state,
    resource_limit_descriptor, service_unavailable_descriptor, timeout_descriptor,
    CassieErrorDescriptor,
};
pub(super) fn execution_descriptor(message: &str) -> CassieErrorDescriptor {
    if ["VALUES types ", "GREATEST types ", "LEAST types "]
        .iter()
        .any(|prefix| message.starts_with(prefix))
        && message.ends_with(" cannot be matched")
    {
        return bad_request_descriptor("42804", message.to_string());
    }
    if message.starts_with("table \"")
        && message.contains(" columns available but ")
        && message.ends_with(" columns specified")
    {
        return bad_request_descriptor("42P10", message.to_string());
    }
    if message.starts_with("column \"") && message.ends_with("\" specified more than once") {
        return bad_request_descriptor("42701", message.to_owned());
    }
    if message == "negative substring length not allowed" {
        return bad_request_descriptor("22011", message.to_owned());
    }
    let pagination_state = pagination_sql_state(message);
    if let Some(sql_state) = pagination_state {
        return bad_request_descriptor(sql_state, message.to_string());
    }
    if message.eq_ignore_ascii_case("query canceled") {
        return cancellation_descriptor();
    }
    if message.eq_ignore_ascii_case("query timeout exceeded") {
        return timeout_descriptor();
    }
    if let Some(state) = expression_sql_state(message) {
        return bad_request_descriptor(state, message.to_owned());
    }
    if scalar_input_syntax(message) {
        return bad_request_descriptor("22P02", message.to_string());
    }
    if scalar_input_range(message) {
        return bad_request_descriptor("22003", message.to_string());
    }
    if boolean_argument_error(message) {
        return bad_request_descriptor("42804", message.to_string());
    }
    if [
        "DISTINCT specified, but ",
        "ORDER BY specified, but ",
        "FILTER specified, but ",
    ]
    .iter()
    .any(|prefix| message.starts_with(prefix))
        && message.ends_with(" is not an aggregate function")
    {
        return bad_request_descriptor("42809", message.to_string());
    }
    if message == "aggregate functions are not allowed in VALUES" {
        return bad_request_descriptor("42803", message.to_string());
    }
    if message == "in an aggregate with DISTINCT, ORDER BY expressions must appear in argument list"
    {
        return bad_request_descriptor("42P10", message.to_string());
    }
    if message.eq_ignore_ascii_case("aggregate integer overflow")
        || matches!(
            message,
            "smallint out of range" | "integer out of range" | "value overflows numeric format"
        )
        || message.eq_ignore_ascii_case(crate::types::numeric::FLOAT_OVERFLOW)
        || message.eq_ignore_ascii_case(crate::executor::filter::BIGINT_OUT_OF_RANGE)
    {
        return bad_request_descriptor("22003", message.to_string());
    }
    if message.starts_with("function ") && message.ends_with(") does not exist") {
        return bad_request_descriptor("42883", message.to_string());
    }
    if message.starts_with("function ") && message.ends_with("(unknown) is not unique") {
        return bad_request_descriptor("42725", message.to_owned());
    }
    if message.starts_with("operator is not unique: ") {
        return bad_request_descriptor("42725", message.to_string());
    }
    if message.starts_with("operator does not exist: ") {
        return bad_request_descriptor("42883", message.to_string());
    }
    if message.starts_with("could not identify an equality operator for type ") {
        return bad_request_descriptor("42883", message.to_string());
    }
    if message.eq_ignore_ascii_case("query admission exhausted") {
        return service_unavailable_descriptor("53300", message.to_string());
    }
    if message.starts_with("query memory budget exceeded:") {
        return resource_limit_descriptor(message.to_string());
    }
    if message.starts_with("query result row limit exceeded:") {
        return resource_limit_descriptor(message.to_string());
    }
    bad_request_descriptor("22000", message.to_string())
}

fn scalar_input_range(message: &str) -> bool {
    message.starts_with("value \"") && message.contains(" is out of range for type ")
        || message.starts_with('"')
            && message.ends_with(" is out of range for type double precision")
}

fn scalar_input_syntax(message: &str) -> bool {
    if matches!(
        message,
        "invalid input syntax for type smallint"
            | "invalid input syntax for type integer"
            | "invalid input syntax for type bigint"
    ) {
        return true;
    }
    message
        .strip_prefix("invalid input syntax for type ")
        .is_some_and(|tail| {
            [
                "boolean:",
                "smallint:",
                "integer:",
                "bigint:",
                "double precision:",
                "uuid:",
            ]
            .iter()
            .any(|prefix| tail.starts_with(prefix))
        })
}

fn boolean_argument_error(message: &str) -> bool {
    [
        "FILTER",
        "IS TRUE",
        "IS NOT TRUE",
        "IS FALSE",
        "IS NOT FALSE",
        "IS UNKNOWN",
        "IS NOT UNKNOWN",
    ]
    .iter()
    .any(|name| {
        message
            .strip_prefix("argument of ")
            .and_then(|tail| tail.strip_prefix(name))
            .is_some_and(|tail| tail.starts_with(" must be type boolean, not type "))
    })
}

fn expression_sql_state(message: &str) -> Option<&'static str> {
    match message {
        "cannot take square root of a negative number"
        | "zero raised to a negative power is undefined"
        | "a negative number raised to a non-integer power yields a complex result" => {
            Some("2201F")
        }
        "invalid escape string" | "LIKE pattern must not end with escape character" => {
            Some("22025")
        }
        _ if message.eq_ignore_ascii_case("division by zero") => Some("22012"),
        _ => None,
    }
}
