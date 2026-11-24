//! Borrowed ESCAPE matching models native `like_escape` followed by strict LIKE.
use super::{eval_scalar_with_context, EvalContext, Expr, QueryError, RowAccess, ScalarValue};

pub(super) fn validate_rhs(pattern: &ScalarValue, escape: &ScalarValue) -> Result<(), QueryError> {
    if matches!(pattern, ScalarValue::Null) || matches!(escape, ScalarValue::Null) {
        return Ok(());
    }
    escape_character(escape).map(|_| ())
}

fn escape_character(escape: &ScalarValue) -> Result<Option<char>, QueryError> {
    let ScalarValue::Str(escape) = escape else {
        return Err(QueryError::General("LIKE ESCAPE requires text".into()));
    };
    let mut characters = escape.chars();
    let first = characters.next();
    if characters.next().is_some() {
        return Err(QueryError::General("invalid escape string".into()));
    }
    Ok(first)
}

pub(super) fn scalar(
    value: &ScalarValue,
    pattern: &ScalarValue,
    escape: &ScalarValue,
    case_insensitive: bool,
    check: impl FnMut() -> Result<(), QueryError>,
) -> Result<ScalarValue, QueryError> {
    validate_rhs(pattern, escape)?;
    if matches!(value, ScalarValue::Null)
        || matches!(pattern, ScalarValue::Null)
        || matches!(escape, ScalarValue::Null)
    {
        return Ok(ScalarValue::Null);
    }
    let (ScalarValue::Str(value), ScalarValue::Str(pattern)) = (value, pattern) else {
        return Err(QueryError::General("LIKE requires text operands".into()));
    };
    super::like::controlled(
        value,
        pattern,
        escape_character(escape)?,
        case_insensitive,
        check,
    )
    .map(ScalarValue::Bool)
    .map_err(|error| match error {
        super::like::MatchError::Pattern => {
            QueryError::General(super::like::LikePatternError.to_string())
        }
        super::like::MatchError::Interrupted(error) => error,
    })
}

pub(super) fn runtime<R: RowAccess + ?Sized>(
    row: &R,
    expression: &Expr,
    context: EvalContext<'_>,
) -> Result<ScalarValue, QueryError> {
    if let Some(value) = super::constants::value(expression, row, context)? {
        return Ok(value);
    }
    let Expr::Like {
        expr,
        pattern,
        escape,
        case_insensitive,
    } = expression
    else {
        unreachable!("LIKE dispatch");
    };
    let value = eval_scalar_with_context(row, expr, context)?;
    let pattern = eval_scalar_with_context(row, pattern, context)?;
    let escape = eval_scalar_with_context(row, escape, context)?;
    scalar(&value, &pattern, &escape, *case_insensitive, || {
        context.session.map_or(Ok(()), |session| {
            session
                .check_statement_interruption()
                .map_err(QueryError::from)
        })
    })
}
