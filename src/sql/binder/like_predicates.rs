//! Resolve selected textual LIKE operands and native helper coercions.
use super::{coalesce_results::ResultTypes, CassieError, DataType, Expr};

pub(super) fn validate(
    expr: &mut Expr,
    pattern: &mut Expr,
    escape: &mut Expr,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    let kinds = [
        types.expression_type(expr),
        types.expression_type(pattern),
        types.expression_type(escape),
    ];
    if kinds.iter().flatten().any(|kind| {
        matches!(
            kind,
            DataType::Array(_) | DataType::Json | DataType::Vector(_) | DataType::Bytea
        )
    }) {
        return Err(CassieError::Unsupported(
            "LIKE ESCAPE non-text native contracts remain pending under #793/#796".into(),
        ));
    }
    if kinds[1..].iter().flatten().any(|kind| !textual(kind)) {
        return Err(CassieError::Execution(format!(
            "function pg_catalog.like_escape({}, {}) does not exist",
            operand_label(pattern, kinds[1].as_ref()),
            operand_label(escape, kinds[2].as_ref())
        )));
    }
    if kinds[0].as_ref().is_some_and(|kind| !textual(kind)) {
        return Err(CassieError::Execution(format!(
            "operator does not exist: {} ~~ text",
            label(kinds[0].as_ref())
        )));
    }
    for (operand, kind) in [(pattern, &kinds[1]), (escape, &kinds[2])] {
        if matches!(kind, Some(DataType::Char { .. })) {
            let input = std::mem::replace(operand, Expr::Null);
            *operand = Expr::Cast {
                expr: Box::new(input),
                data_type: DataType::Text,
                source_type: kind.clone(),
                identity: crate::sql::ast::CastIdentity::Scalar,
            };
        }
    }
    Ok(())
}

fn textual(kind: &DataType) -> bool {
    matches!(
        kind,
        DataType::Null | DataType::Text | DataType::Char { .. } | DataType::Varchar { .. }
    )
}
fn label(kind: Option<&DataType>) -> String {
    match kind {
        None | Some(DataType::Null) => "unknown".into(),
        Some(DataType::Int) => "integer".into(),
        Some(DataType::Float) => "double precision".into(),
        Some(DataType::Char { .. }) => "character".into(),
        Some(DataType::Varchar { .. }) => "character varying".into(),
        Some(kind) => kind.type_name(),
    }
}

fn operand_label(expr: &Expr, kind: Option<&DataType>) -> String {
    if matches!(expr, Expr::StringLiteral(_) | Expr::Null) {
        "unknown".into()
    } else {
        label(kind)
    }
}
