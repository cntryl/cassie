//! `INSERT ... ON CONFLICT [target] DO NOTHING | DO UPDATE SET ... [WHERE ...]`.

use super::expr::{parse_expression, split_csv};
use super::identifiers::normalize_column_identifier;
use super::lexical::{matching_paren, pattern_end, trim_separators};
use super::{find_top_level_keyword, Expr, SqlError};
use crate::sql::ast::{InsertConflictAction, InsertConflictClause};

pub(super) fn split_insert_on_conflict(
    raw: &str,
) -> Result<(&str, Option<InsertConflictClause>), SqlError> {
    let Some(pos) = find_top_level_keyword(raw, 0, "on conflict") else {
        return Ok((raw, None));
    };
    let statement = raw[..pos].trim();
    let clause = raw[pos..].trim();
    Ok((statement, Some(parse_on_conflict_clause(clause)?)))
}

#[derive(Default)]
struct ConflictTarget {
    fields: Vec<String>,
    expressions: Vec<Expr>,
    constraint: Option<String>,
    predicate: Option<Expr>,
}

fn parse_on_conflict_clause(raw: &str) -> Result<InsertConflictClause, SqlError> {
    let end = pattern_end(raw, 0, "on conflict")
        .ok_or_else(|| SqlError::new("invalid ON CONFLICT clause".into()))?;
    let remainder = trim_separators(&raw[end..]);
    let do_pos = find_top_level_keyword(remainder, 0, "do")
        .ok_or_else(|| SqlError::new("ON CONFLICT requires DO clause".into()))?;
    let target = parse_conflict_target(trim_separators(&remainder[..do_pos]))?;
    let (action, update_row_subqueries) = parse_on_conflict_action(remainder[do_pos + 2..].trim())?;
    if matches!(action, InsertConflictAction::DoUpdate { .. })
        && target.fields.is_empty()
        && target.expressions.is_empty()
        && target.constraint.is_none()
    {
        return Err(SqlError::syntax(
            "ON CONFLICT DO UPDATE requires inference specification or constraint name".into(),
        ));
    }
    Ok(InsertConflictClause {
        target_fields: target.fields,
        action,
        constraint: target.constraint,
        target_expressions: target.expressions,
        target_predicate: target.predicate,
        update_row_subqueries,
    })
}

fn parse_conflict_target(raw: &str) -> Result<ConflictTarget, SqlError> {
    let mut target = ConflictTarget::default();
    if raw.is_empty() {
        return Ok(target);
    }
    if let Some(end) = pattern_end(raw, 0, "on constraint") {
        let name = trim_separators(&raw[end..]);
        if name.is_empty() || name.split_whitespace().count() != 1 {
            return Err(SqlError::syntax(
                "ON CONFLICT ON CONSTRAINT requires one constraint name".into(),
            ));
        }
        target.constraint = Some(super::identifiers::normalize_identifier(name)?);
        return Ok(target);
    }
    if !raw.starts_with('(') {
        return Err(SqlError::new(
            "ON CONFLICT target must be parenthesized".into(),
        ));
    }
    let close = matching_paren(raw, 0)
        .ok_or_else(|| SqlError::new("ON CONFLICT target must be parenthesized".into()))?;
    for element in split_csv(&raw[1..close]) {
        let element = trim_separators(element);
        if element.is_empty() {
            return Err(SqlError::new(
                "ON CONFLICT target field cannot be empty".into(),
            ));
        }
        match parse_inference_element(element)? {
            InferenceElement::Column(field) => target.fields.push(field),
            InferenceElement::Expression(expr) => target.expressions.push(expr),
        }
    }
    let rest = trim_separators(&raw[close + 1..]);
    if !rest.is_empty() {
        let where_end = pattern_end(rest, 0, "where").ok_or_else(|| {
            SqlError::syntax(format!(
                "syntax error at or near \"{}\"",
                rest.split_whitespace().next().unwrap_or_default()
            ))
        })?;
        target.predicate = Some(parse_expression(&rest[where_end..])?);
    }
    Ok(target)
}

enum InferenceElement {
    Column(String),
    Expression(Expr),
}

/// `column [COLLATE c] [opclass]` or `(expression) [COLLATE c] [opclass]`,
/// where a function call may also stand without its own parentheses.
fn parse_inference_element(raw: &str) -> Result<InferenceElement, SqlError> {
    let (expression, rest) = if raw.starts_with('(') {
        let close = matching_paren(raw, 0)
            .ok_or_else(|| SqlError::syntax("unbalanced ON CONFLICT expression".into()))?;
        (Some(parse_expression(&raw[1..close])?), &raw[close + 1..])
    } else if let Some(open) = function_call_open(raw) {
        let close = matching_paren(raw, open)
            .ok_or_else(|| SqlError::syntax("unbalanced ON CONFLICT expression".into()))?;
        (Some(parse_expression(&raw[..=close])?), &raw[close + 1..])
    } else {
        (None, raw)
    };
    if let Some(expression) = expression {
        validate_element_modifiers(trim_separators(rest))?;
        return Ok(InferenceElement::Expression(expression));
    }
    let column_end = identifier_end(raw);
    let column = normalize_column_identifier(&raw[..column_end])?;
    validate_element_modifiers(trim_separators(&raw[column_end..]))?;
    Ok(InferenceElement::Column(column))
}

/// The opening parenthesis of `name(...)` when `raw` begins with a call.
fn function_call_open(raw: &str) -> Option<usize> {
    let open = raw.find('(')?;
    let name = raw[..open].trim_end();
    (!name.is_empty()
        && !name.contains(char::is_whitespace)
        && name
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '.')))
    .then_some(open)
}

fn identifier_end(raw: &str) -> usize {
    let mut quoted = false;
    for (index, character) in raw.char_indices() {
        if character == '"' {
            quoted = !quoted;
        } else if !quoted && character.is_whitespace() {
            return index;
        }
    }
    raw.len()
}

/// Accepts `[COLLATE collation] [opclass]`; the operator class only selects
/// among indexes on the same key, which Cassie's B-tree keys share.
fn validate_element_modifiers(raw: &str) -> Result<(), SqlError> {
    let mut words = raw.split_whitespace().peekable();
    if words
        .peek()
        .is_some_and(|word| word.eq_ignore_ascii_case("collate"))
    {
        let _ = words.next();
        if words.next().is_none() {
            return Err(SqlError::syntax(
                "syntax error at end of input: COLLATE requires a name".into(),
            ));
        }
    }
    if let Some(opclass) = words.next() {
        let valid = opclass.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|character| character.is_alphanumeric() || matches!(character, '_' | '"'))
        });
        if !valid {
            return Err(SqlError::syntax(format!(
                "syntax error at or near \"{opclass}\""
            )));
        }
    }
    if let Some(extra) = words.next() {
        return Err(SqlError::syntax(format!(
            "syntax error at or near \"{extra}\""
        )));
    }
    Ok(())
}

type ParsedConflictAction = (
    InsertConflictAction,
    Vec<crate::sql::ast::RowSubqueryAssignment>,
);

fn parse_on_conflict_action(raw: &str) -> Result<ParsedConflictAction, SqlError> {
    let raw = trim_separators(raw);
    if raw.eq_ignore_ascii_case("nothing") {
        return Ok((InsertConflictAction::DoNothing, Vec::new()));
    }
    let end = pattern_end(raw, 0, "update set")
        .ok_or_else(|| SqlError::new("ON CONFLICT supports DO NOTHING or DO UPDATE SET".into()))?;
    let after_set = trim_separators(&raw[end..]);
    let where_pos = find_top_level_keyword(after_set, 0, "where");
    let parsed =
        super::dml_targets::parse_set_list(&after_set[..where_pos.unwrap_or(after_set.len())])?;
    if parsed.assignments.is_empty() && parsed.row_subqueries.is_empty() {
        return Err(SqlError::new(
            "ON CONFLICT DO UPDATE SET requires at least one assignment".into(),
        ));
    }
    let filter = where_pos
        .map(|position| {
            let raw = after_set[position + 5..].trim();
            if raw.is_empty() {
                Err(SqlError::syntax(
                    "syntax error at end of input: WHERE requires a condition".into(),
                ))
            } else {
                parse_expression(raw)
            }
        })
        .transpose()?;
    Ok((
        InsertConflictAction::DoUpdate {
            assignments: parsed.assignments,
            filter,
        },
        parsed.row_subqueries,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clause(raw: &str) -> InsertConflictClause {
        parse_on_conflict_clause(raw).expect("parse ON CONFLICT")
    }

    #[test]
    fn should_parse_each_inference_element_given_operator_classes() {
        // Arrange
        let raw = "ON CONFLICT (email pg_catalog.text_ops, (lower(token)) text_ops) \
                   DO UPDATE SET payload = excluded.payload";

        // Act
        let parsed = clause(raw);

        // Assert
        assert_eq!(parsed.target_fields, vec!["email"]);
        assert_eq!(parsed.target_expressions.len(), 1);
        assert!(parsed.constraint.is_none());
    }

    #[test]
    fn should_parse_a_partial_index_predicate_given_a_where_target() {
        // Arrange
        let raw = "ON CONFLICT (lower(email)) WHERE active DO UPDATE SET version = 1 \
                   WHERE target.version < excluded.version";

        // Act
        let parsed = clause(raw);

        // Assert
        assert_eq!(parsed.target_fields, Vec::<String>::new());
        assert_eq!(parsed.target_expressions.len(), 1);
        assert!(parsed.target_predicate.is_some());
        assert!(matches!(
            parsed.action,
            InsertConflictAction::DoUpdate {
                filter: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn should_parse_a_named_constraint_target() {
        // Arrange
        let raw = "ON CONFLICT ON CONSTRAINT records_pkey DO NOTHING";

        // Act
        let parsed = clause(raw);

        // Assert
        assert_eq!(parsed.constraint.as_deref(), Some("records_pkey"));
        assert!(matches!(parsed.action, InsertConflictAction::DoNothing));
    }

    #[test]
    fn should_reject_do_update_without_an_inference_target() {
        // Arrange
        let raw = "ON CONFLICT DO UPDATE SET a = 1";

        // Act
        let rejected = parse_on_conflict_clause(raw);

        // Assert
        assert!(rejected.is_err());
    }
}
