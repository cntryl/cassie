//! Shared DML clause parsing: target aliases, tuple assignments, joined
//! mutation sources and `RETURNING WITH (OLD AS ..., NEW AS ...)`.

use super::expr::{parse_expression, split_csv};
use super::identifiers::{normalize_column_identifier, normalize_identifier};
use super::lexical::{matching_paren, pattern_end, trim_separators};
use super::query::parse_projection_items;
use super::{find_top_level_keyword, parse_statement, Expr, QueryStatement, SelectItem, SqlError};
use crate::sql::ast::{FunctionCall, ReturningAliases, RowSubqueryAssignment};

/// Keywords that end a target relation and cannot be its bare alias.
const RESERVED_AFTER_TARGET: &[&str] = &[
    "set",
    "using",
    "where",
    "returning",
    "on",
    "values",
    "select",
    "default",
    "overriding",
    "when",
];

/// Splits an optional `[AS] alias` from the start of `tail`.
pub(super) fn split_target_alias(tail: &str) -> Result<(Option<String>, &str), SqlError> {
    let mut rest = trim_separators(tail);
    let explicit = pattern_end(rest, 0, "as");
    if let Some(end) = explicit {
        rest = trim_separators(&rest[end..]);
    }
    let end = alias_end(rest);
    if end == 0 {
        if explicit.is_some() {
            return Err(SqlError::syntax(
                "syntax error at or near \"AS\": missing alias".into(),
            ));
        }
        return Ok((None, rest));
    }
    let word = &rest[..end];
    if explicit.is_none()
        && RESERVED_AFTER_TARGET
            .iter()
            .any(|keyword| word.eq_ignore_ascii_case(keyword))
    {
        return Ok((None, rest));
    }
    Ok((
        Some(normalize_identifier(word)?),
        trim_separators(&rest[end..]),
    ))
}

fn alias_end(raw: &str) -> usize {
    if raw.starts_with('"') {
        let mut chars = raw.char_indices().skip(1).peekable();
        while let Some((index, character)) = chars.next() {
            if character == '"' {
                if chars.peek().is_some_and(|(_, next)| *next == '"') {
                    let _ = chars.next();
                    continue;
                }
                return index + 1;
            }
        }
        return 0;
    }
    raw.char_indices()
        .find(|(_, character)| !(character.is_alphanumeric() || matches!(character, '_' | '$')))
        .map_or(raw.len(), |(index, _)| index)
}

/// The function name that marks a `column = DEFAULT` assignment.
pub(crate) const DEFAULT_ASSIGNMENT: &str = crate::sql::ast::DEFAULT_ASSIGNMENT_MARKER;

#[derive(Debug, Default)]
pub(super) struct ParsedAssignments {
    pub(super) assignments: Vec<(String, Expr)>,
    pub(super) row_subqueries: Vec<RowSubqueryAssignment>,
}

/// Parses `SET` items, including `(a, b) = [ROW] (x, y)`, `(a, b) = (SELECT ...)`
/// and `column = DEFAULT`.
pub(super) fn parse_set_list(raw: &str) -> Result<ParsedAssignments, SqlError> {
    let mut parsed = ParsedAssignments::default();
    for item in split_csv(raw) {
        let item = trim_separators(item);
        if item.is_empty() {
            return Err(SqlError::new(
                "UPDATE SET cannot include empty assignment".into(),
            ));
        }
        let eq = super::dml::split_top_level_assignment(item)
            .ok_or_else(|| SqlError::new("UPDATE SET assignments require '='".into()))?;
        let left = trim_separators(&item[..eq]);
        let right = trim_separators(&item[eq + 1..]);
        if right.is_empty() {
            return Err(SqlError::new("UPDATE SET assignment missing value".into()));
        }
        if left.starts_with('(') {
            parse_tuple_assignment(left, right, &mut parsed)?;
            continue;
        }
        if left.is_empty() {
            return Err(SqlError::new(
                "UPDATE SET assignment missing column name".into(),
            ));
        }
        parsed
            .assignments
            .push((normalize_column_identifier(left)?, assignment_value(right)?));
    }
    Ok(parsed)
}

fn assignment_value(raw: &str) -> Result<Expr, SqlError> {
    if raw.eq_ignore_ascii_case("default") {
        return Ok(default_marker());
    }
    parse_expression(raw)
}

#[must_use]
pub(crate) fn default_marker() -> Expr {
    Expr::Function(Box::new(FunctionCall {
        name: DEFAULT_ASSIGNMENT.to_string(),
        args: Vec::new(),
        quantifier: crate::sql::ast::AggregateQuantifier::default(),
        filter: None,
        order_by: Vec::new(),
    }))
}

fn parse_tuple_assignment(
    left: &str,
    right: &str,
    parsed: &mut ParsedAssignments,
) -> Result<(), SqlError> {
    let close = matching_paren(left, 0)
        .filter(|close| *close + 1 == left.len())
        .ok_or_else(|| SqlError::syntax("malformed SET column list".into()))?;
    let columns = split_csv(&left[1..close])
        .into_iter()
        .map(|column| normalize_column_identifier(trim_separators(column)))
        .collect::<Result<Vec<_>, _>>()?;
    let values = pattern_end(right, 0, "row").map_or(right, |end| trim_separators(&right[end..]));
    let close = matching_paren(values, 0)
        .filter(|close| values.starts_with('(') && *close + 1 == values.len())
        .ok_or_else(|| {
            SqlError::syntax(
                "source for a multiple-column UPDATE item must be a sub-SELECT or ROW() expression"
                    .into(),
            )
        })?;
    let inner = trim_separators(&values[1..close]);
    let is_row_constructor = values.len() != right.len();
    if !is_row_constructor
        && (pattern_end(inner, 0, "select").is_some() || pattern_end(inner, 0, "with").is_some())
    {
        let query = parse_statement(inner)?;
        if !matches!(query.statement, QueryStatement::Select(_)) {
            return Err(SqlError::syntax(
                "multiple-column assignment requires a SELECT".into(),
            ));
        }
        parsed.row_subqueries.push(RowSubqueryAssignment {
            columns,
            query: Box::new(query),
        });
        return Ok(());
    }
    let expressions = split_csv(inner)
        .into_iter()
        .map(|value| assignment_value(trim_separators(value)))
        .collect::<Result<Vec<_>, _>>()?;
    if expressions.len() != columns.len() {
        return Err(SqlError::syntax(
            "number of columns does not match number of values".into(),
        ));
    }
    parsed
        .assignments
        .extend(columns.into_iter().zip(expressions));
    Ok(())
}

/// Splits `raw` at its top-level `RETURNING` and parses the clause.
pub(super) fn split_returning(
    raw: &str,
) -> Result<(&str, Vec<SelectItem>, Option<ReturningAliases>), SqlError> {
    let raw = raw.trim();
    let Some(position) = find_top_level_keyword(raw, 0, "returning") else {
        return Ok((raw, Vec::new(), None));
    };
    let (items, aliases) = parse_returning_items(&raw[position + "returning".len()..])?;
    Ok((raw[..position].trim(), items, aliases))
}

/// Parses `[WITH (OLD AS o, NEW AS n)] items`.
pub(super) fn parse_returning_items(
    raw: &str,
) -> Result<(Vec<SelectItem>, Option<ReturningAliases>), SqlError> {
    let raw = trim_separators(raw);
    let Some(with_end) = pattern_end(raw, 0, "with") else {
        return Ok((parse_projection_items(raw)?, None));
    };
    let options = trim_separators(&raw[with_end..]);
    let close = matching_paren(options, 0)
        .filter(|_| options.starts_with('('))
        .ok_or_else(|| SqlError::syntax("syntax error at or near \"WITH\"".into()))?;
    let mut aliases = ReturningAliases::default();
    let mut seen = (false, false);
    for option in split_csv(&options[1..close]) {
        let option = trim_separators(option);
        let (kind, alias) = super::clauses::split_top_level(option, "as")
            .ok_or_else(|| SqlError::syntax(format!("syntax error at or near \"{option}\"")))?;
        let alias = normalize_identifier(trim_separators(alias))?;
        match trim_separators(kind).to_ascii_lowercase().as_str() {
            "old" if !seen.0 => {
                seen.0 = true;
                aliases.old = alias;
            }
            "new" if !seen.1 => {
                seen.1 = true;
                aliases.new = alias;
            }
            "old" | "new" => {
                return Err(SqlError::syntax(format!(
                    "{} cannot be specified multiple times",
                    trim_separators(kind).to_ascii_uppercase()
                )));
            }
            _ => {
                return Err(SqlError::syntax(format!(
                    "syntax error at or near \"{}\"",
                    trim_separators(kind)
                )));
            }
        }
    }
    let items = parse_projection_items(&options[close + 1..])?;
    Ok((items, Some(aliases)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_expand_a_row_constructor_into_column_assignments() {
        // Arrange
        let raw = "(label, version) = ROW(excluded.label, excluded.version), note = DEFAULT";

        // Act
        let parsed = parse_set_list(raw).expect("parse tuple assignment");

        // Assert
        assert_eq!(
            parsed
                .assignments
                .iter()
                .map(|(column, _)| column.as_str())
                .collect::<Vec<_>>(),
            vec!["label", "version", "note"]
        );
        assert!(matches!(
            &parsed.assignments[2].1,
            Expr::Function(function) if function.name == DEFAULT_ASSIGNMENT
        ));
        assert_eq!(parsed.row_subqueries.len(), 0);
    }

    #[test]
    fn should_keep_a_tuple_subquery_as_one_row_assignment() {
        // Arrange
        let raw = "(label, version) = (SELECT s.label, s.version FROM s WHERE s.id = 1)";

        // Act
        let parsed = parse_set_list(raw).expect("parse subquery tuple assignment");

        // Assert
        assert_eq!(parsed.assignments.len(), 0);
        assert_eq!(parsed.row_subqueries.len(), 1);
        assert_eq!(parsed.row_subqueries[0].columns, vec!["label", "version"]);
    }

    #[test]
    fn should_reject_a_tuple_assignment_with_mismatched_arity() {
        // Arrange
        let raw = "(a, b) = (1, 2, 3)";

        // Act
        let rejected = parse_set_list(raw);

        // Assert
        assert!(rejected.is_err());
    }

    #[test]
    fn should_split_a_target_alias_before_a_clause_keyword() {
        // Arrange
        let tail = "AS target SET label = 1";

        // Act
        let (alias, rest) = split_target_alias(tail).expect("split alias");
        let (bare, bare_rest) = split_target_alias("SET label = 1").expect("no alias");

        // Assert
        assert_eq!(alias.as_deref(), Some("target"));
        assert_eq!(rest, "SET label = 1");
        assert_eq!(bare, None);
        assert_eq!(bare_rest, "SET label = 1");
    }

    #[test]
    fn should_parse_returning_row_image_aliases() {
        // Arrange
        let raw = "WITH (OLD AS o, NEW AS n) o.id, n.id";

        // Act
        let (items, aliases) = parse_returning_items(raw).expect("parse RETURNING WITH");

        // Assert
        assert_eq!(items.len(), 2);
        assert_eq!(
            aliases,
            Some(ReturningAliases {
                old: "o".to_string(),
                new: "n".to_string(),
            })
        );
    }
}
