//! Named argument mapping for selected regular-expression scalar functions.

use super::expr::parse_expression;
use super::identifiers::normalize_identifier;
use super::{Expr, SqlError};

pub(super) fn arguments(name: &str, body: &str) -> Result<Option<(String, Vec<Expr>)>, SqlError> {
    let Some(function) = crate::sql::functions::function(name) else {
        return Ok(None);
    };
    let argument_names: &[&str] = match function.id {
        crate::sql::functions::FunctionId::RegexpLike => &["string", "pattern", "flags"],
        crate::sql::functions::FunctionId::RegexpCount => &["string", "pattern", "start", "flags"],
        crate::sql::functions::FunctionId::RegexpReplace => {
            &["string", "pattern", "replacement", "start", "N", "flags"]
        }
        _ => return Ok(None),
    };
    let raw_arguments = super::expr::split_csv(body);
    let Some(arguments) = map_named_arguments(function.name, argument_names, raw_arguments)? else {
        return Ok(None);
    };
    let ordered = ordered_named_arguments(function.id, function.name, argument_names, arguments)?;
    Ok(Some((function.name.to_owned(), ordered)))
}

fn map_named_arguments(
    function_name: &str,
    argument_names: &[&str],
    raw_arguments: Vec<&str>,
) -> Result<Option<Vec<Option<Expr>>>, SqlError> {
    let has_named_argument = raw_arguments
        .iter()
        .any(|argument| split_named_argument(argument).is_some());
    if !has_named_argument {
        return Ok(None);
    }
    if raw_arguments
        .iter()
        .any(|argument| split_named_argument(argument).is_none())
    {
        return Err(SqlError::syntax(
            "cannot mix positional and named function arguments".into(),
        ));
    }

    let mut arguments = vec![None; argument_names.len()];
    for argument in raw_arguments {
        let (name, value) =
            split_named_argument(argument).expect("named-argument presence was checked above");
        let name = normalize_identifier(name)?;
        let Some(index) = argument_names.iter().position(|expected| *expected == name) else {
            return Err(SqlError::syntax(format!(
                "unknown named argument '{name}' for function '{function_name}'"
            )));
        };
        if arguments[index].is_some() {
            return Err(SqlError::syntax(format!(
                "duplicate named argument '{name}' for function '{function_name}'"
            )));
        }
        if value.trim().is_empty() {
            return Err(SqlError::syntax(format!(
                "named argument '{name}' requires a value"
            )));
        }
        arguments[index] = Some(parse_expression(value)?);
    }

    Ok(Some(arguments))
}

fn ordered_named_arguments(
    function_id: crate::sql::functions::FunctionId,
    function_name: &str,
    argument_names: &[&str],
    mut arguments: Vec<Option<Expr>>,
) -> Result<Vec<Expr>, SqlError> {
    if arguments[0].is_none() || arguments[1].is_none() {
        return Err(SqlError::syntax(format!(
            "function '{function_name}' requires named arguments 'string' and 'pattern'"
        )));
    }
    if function_id == crate::sql::functions::FunctionId::RegexpReplace && arguments[2].is_none() {
        return Err(SqlError::syntax(format!(
            "function '{function_name}' requires named argument 'replacement'"
        )));
    }
    if function_id == crate::sql::functions::FunctionId::RegexpCount
        && arguments[2].is_none()
        && arguments[3].is_some()
    {
        return Err(SqlError::undefined_function(format!(
            "function {function_name}(text, text, text) does not exist"
        )));
    }
    if function_id == crate::sql::functions::FunctionId::RegexpReplace
        && arguments[3].is_none()
        && arguments[4].is_none()
        && arguments[5].is_some()
    {
        let ordered = [0, 1, 2, 5]
            .into_iter()
            .map(|index| {
                arguments[index]
                    .take()
                    .expect("required named regexp_replace arguments were checked above")
            })
            .collect();
        return Ok(ordered);
    }

    let last_argument = arguments
        .iter()
        .rposition(Option::is_some)
        .expect("required arguments were checked above");
    let mut ordered = Vec::with_capacity(last_argument + 1);
    for (index, argument) in arguments.into_iter().enumerate().take(last_argument + 1) {
        let Some(argument) = argument else {
            return Err(SqlError::syntax(format!(
                "missing named argument '{}' for function '{function_name}'",
                argument_names[index]
            )));
        };
        ordered.push(argument);
    }

    Ok(ordered)
}

fn split_named_argument(raw: &str) -> Option<(&str, &str)> {
    super::clauses::find_top_level_keyword_span(raw, 0, "=>")
        .map(|(position, end)| (raw[..position].trim(), raw[end..].trim()))
}

#[cfg(test)]
mod tests {
    use super::arguments;
    use crate::sql::ast::Expr;

    #[test]
    fn should_order_named_regexp_count_arguments_by_signature() {
        // Arrange
        let name = "regexp_count";
        let body = "flags => 'i', start => 4, pattern => 'a.', string => $1";

        // Act
        let (_, arguments) = arguments(name, body)
            .expect("named arguments are valid")
            .expect("regexp_count uses the named-argument parser");

        // Assert
        assert!(matches!(arguments[0], Expr::Param(0)));
        assert!(matches!(arguments[1], Expr::StringLiteral(ref pattern) if pattern == "a."));
        assert!(matches!(arguments[2], Expr::IntegerLiteral(4)));
        assert!(matches!(arguments[3], Expr::StringLiteral(ref flags) if flags == "i"));
    }

    #[test]
    fn should_reject_named_regexp_count_flags_without_start() {
        // Arrange
        let name = "regexp_count";
        let body = "string => 'value', pattern => 'value', flags => 'i'";

        // Act
        let error = arguments(name, body).expect_err("flags require an explicit start");

        // Assert
        assert_eq!(error.kind(), crate::sql::SqlErrorKind::UndefinedFunction);
        assert!(error.to_string().ends_with(") does not exist"));
    }

    #[test]
    fn should_reject_unknown_named_regexp_like_arguments() {
        // Arrange
        let name = "regexp_like";
        let body = "string => 'value', pattern => 'value', mode => 'i'";

        // Act
        let error = arguments(name, body).expect_err("unknown named arguments are rejected");

        // Assert
        assert!(error.to_string().contains("unknown named argument 'mode'"));
    }

    #[test]
    fn should_place_named_regexp_replace_flags_in_the_short_overload() {
        // Arrange
        let name = "regexp_replace";
        let body = "flags => 'g', replacement => 'X', pattern => 'a', string => 'abc'";

        // Act
        let (_, arguments) = arguments(name, body)
            .expect("named arguments are valid")
            .expect("regexp_replace uses the named-argument parser");

        // Assert
        assert_eq!(arguments.len(), 4);
        assert!(matches!(arguments[0], Expr::StringLiteral(ref value) if value == "abc"));
        assert!(matches!(arguments[1], Expr::StringLiteral(ref value) if value == "a"));
        assert!(matches!(arguments[2], Expr::StringLiteral(ref value) if value == "X"));
        assert!(matches!(arguments[3], Expr::StringLiteral(ref value) if value == "g"));
    }

    #[test]
    fn should_order_named_regexp_replace_start_occurrence_flags() {
        // Arrange
        let name = "regexp_replace";
        let body = "flags => 'i', \"N\" => 2, replacement => 'X', start => 3, pattern => 'a', string => 'abc'";

        // Act
        let (_, arguments) = arguments(name, body)
            .expect("named arguments are valid")
            .expect("regexp_replace uses the named-argument parser");

        // Assert
        assert_eq!(arguments.len(), 6);
        assert!(matches!(arguments[0], Expr::StringLiteral(ref value) if value == "abc"));
        assert!(matches!(arguments[3], Expr::IntegerLiteral(3)));
        assert!(matches!(arguments[4], Expr::IntegerLiteral(2)));
        assert!(matches!(arguments[5], Expr::StringLiteral(ref value) if value == "i"));
    }

    #[test]
    fn should_reject_named_regexp_replace_without_replacement() {
        // Arrange
        let name = "regexp_replace";
        let body = "string => 'abc', pattern => 'a', flags => 'g'";

        // Act
        let error = arguments(name, body).expect_err("replacement is required");

        // Assert
        assert!(error
            .to_string()
            .contains("requires named argument 'replacement'"));
    }
}
