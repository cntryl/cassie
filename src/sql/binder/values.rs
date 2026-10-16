//! Common-column typing for flat VALUES sources over existing scalar identities.

use super::{CassieError, DataType, Expr, FieldSchema, HashMap, Schema};
use crate::catalog::FunctionMeta;

pub(crate) fn names(rows: &[Vec<Expr>]) -> impl Iterator<Item = String> {
    (0..rows.first().map_or(0, Vec::len)).map(|index| format!("column{}", index + 1))
}

pub(crate) fn schema(
    rows: &[Vec<Expr>],
    functions: &HashMap<String, FunctionMeta>,
    params: &[i32],
) -> Result<Schema, CassieError> {
    let width = rows.first().map_or(0, Vec::len);
    if width == 0 || rows.iter().any(|row| row.len() != width) {
        return Err(CassieError::Planner(
            "VALUES lists must all be the same length".into(),
        ));
    }
    let mut fields = Vec::with_capacity(width);
    for (index, name) in names(rows).enumerate() {
        let mut common = DataType::Null;
        let mut actual_types = Vec::with_capacity(rows.len());
        for row in rows {
            let expression = &row[index];
            if matches!(expression, Expr::Null | Expr::StringLiteral(_)) {
                actual_types.push(DataType::Null);
                continue;
            }
            let actual =
                arithmetic::inferred_type(expression, functions, params).unwrap_or(DataType::Null);
            actual_types.push(actual.clone());
            common = common_type(common.clone(), actual.clone()).ok_or_else(|| {
                CassieError::Execution(format!(
                    "VALUES types {} and {} cannot be matched",
                    pg_name(&common),
                    pg_name(&actual)
                ))
            })?;
        }
        if common == DataType::Null {
            common = DataType::Text;
        }
        if actual_types.iter().any(|actual| actual != &common) {
            common = match common {
                DataType::Char { .. } => DataType::Char { length: None },
                DataType::Varchar { .. } => DataType::Varchar { length: None },
                other => other,
            };
        }
        if matches!(common, DataType::Char { length: None }) {
            return Err(CassieError::Unsupported(
                "unconstrained CHAR VALUES columns are not yet supported".into(),
            ));
        }
        fields.push(FieldSchema {
            name,
            data_type: common,
            nullable: true,
        });
    }
    Ok(Schema { fields })
}

fn pg_name(data_type: &DataType) -> String {
    match data_type {
        DataType::Int => "integer".into(),
        DataType::BigInt => "bigint".into(),
        DataType::SmallInt => "smallint".into(),
        DataType::Float => "double precision".into(),
        _ => data_type.type_name(),
    }
}

pub(super) fn coerce(
    rows: &mut [Vec<Expr>],
    functions: &HashMap<String, FunctionMeta>,
    params: &[i32],
) -> Result<(), CassieError> {
    for expression in rows.iter_mut().flatten() {
        validate(expression, false)?;
        arithmetic::bind(expression, functions, params, 0)?;
    }
    let schema = schema(rows, functions, params)?;
    for row in rows {
        for (expression, field) in row.iter_mut().zip(&schema.fields) {
            if matches!(expression, Expr::Param(_)) && params.is_empty() {
                continue;
            }
            if arithmetic::inferred_type(expression, functions, params).as_ref()
                == Some(&field.data_type)
            {
                continue;
            }
            let original = std::mem::replace(expression, Expr::Null);
            *expression = Expr::Cast {
                expr: Box::new(original),
                data_type: field.data_type.clone(),
                source_type: None,
                identity: crate::sql::ast::CastIdentity::Scalar,
            };
        }
    }
    Ok(())
}

#[path = "values_arithmetic.rs"]
mod arithmetic;

#[path = "values_parameters.rs"]
mod parameters;
pub(crate) fn bind_input_types(
    statement: &mut crate::sql::ast::ParsedStatement,
    oids: &[i32],
) -> Result<(), CassieError> {
    parameters::bind(statement, oids)
}
fn validate(expression: &Expr, float_context: bool) -> Result<(), CassieError> {
    match expression {
        Expr::Function(_) | Expr::Exists(_) => {
            return Err(CassieError::Unsupported(
                "functions and subqueries in VALUES are not yet supported".into(),
            ))
        }
        Expr::Column(name) => {
            return Err(CassieError::Planner(format!(
                "unknown column {name:?} in VALUES"
            )))
        }
        Expr::NumericLiteral(_) => {
            return Err(CassieError::Unsupported(
                "unmaterialized NUMERIC VALUES literals are not yet supported".into(),
            ));
        }
        Expr::Cast {
            expr,
            data_type,
            identity,
            ..
        } if matches!(expr.as_ref(), Expr::NumericLiteral(_)) => {
            return super::boolean_contexts::validate_numeric_literal_cast(data_type, *identity);
        }
        Expr::NumberLiteral(_) if !float_context => {
            return Err(CassieError::Unsupported(
                "NUMERIC VALUES literals are not yet supported".into(),
            ))
        }
        Expr::Cast { data_type, .. }
            if !matches!(
                data_type,
                DataType::Null
                    | DataType::Boolean
                    | DataType::SmallInt
                    | DataType::Int
                    | DataType::BigInt
                    | DataType::Float
                    | DataType::Text
                    | DataType::Char { .. }
                    | DataType::Varchar { .. }
            ) =>
        {
            return Err(CassieError::Unsupported(
                "VALUES casts to this type are not yet supported".into(),
            ));
        }
        Expr::Cast {
            data_type: DataType::Float,
            expr,
            ..
        } => return validate(expr, true),
        _ => {}
    }
    expression.try_visit_children(|child| validate(child, float_context))
}

pub(super) fn validate_aliases(source: &super::QuerySource) -> Result<(), CassieError> {
    match source {
        super::QuerySource::Aliased {
            source,
            alias,
            column_aliases,
        } => {
            if let super::QuerySource::Values { rows } = source.as_ref() {
                let available = rows.first().map_or(0, Vec::len);
                if column_aliases.len() > available {
                    let name = crate::sql::ColumnIdentifierPath::parse(alias)
                        .map_err(CassieError::Planner)?
                        .declared_name();
                    return Err(CassieError::Execution(format!("table \"{name}\" has {available} columns available but {} columns specified", column_aliases.len())));
                }
            }
            validate_aliases(source)?;
        }
        super::QuerySource::Join { left, right, .. } => {
            validate_aliases(left)?;
            validate_aliases(right)?;
        }
        _ => {}
    }
    Ok(())
}

fn common_type(left: DataType, right: DataType) -> Option<DataType> {
    if matches!(
        left,
        DataType::Text | DataType::Char { .. } | DataType::Varchar { .. }
    ) && matches!(
        right,
        DataType::Text | DataType::Char { .. } | DataType::Varchar { .. }
    ) {
        return Some(left);
    }
    super::inference::common_case_type(left, right)
}
