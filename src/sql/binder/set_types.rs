//! Static column-type check for set operations.
//!
//! PostgreSQL resolves one common type per output column of `UNION`,
//! `INTERSECT` and `EXCEPT` and rejects operands whose types have none
//! ("UNION types integer and text cannot be matched"). Only types known for
//! certain are compared: base-table columns, casts and numeric or boolean
//! literals. Anything else (untyped string literals, `NULL`, parameters,
//! expressions, derived sources) is left unresolved, as PostgreSQL's
//! `unknown` pseudo-type would coerce it.

use super::wildcard::wildcard_output_fields;
use super::SelectStatement;
use super::{CassieError, Catalog, DataType, Expr, HashMap, QuerySource, SelectItem};
use crate::sql::ast::SetOperator;

/// Rejects a set operation whose left and right operands have a column pair
/// with statically known types in different type categories.
///
/// # Errors
///
/// Returns a planner error naming the operator and both types.
pub(super) fn validate_set_operand_types(
    select: &SelectStatement,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    let Some(set) = &select.set else {
        return Ok(());
    };
    let left = operand_column_types(select, catalog);
    let right = operand_column_types(&set.right, catalog);
    for (left, right) in left.iter().zip(&right) {
        let (Some(left), Some(right)) = (left, right) else {
            continue;
        };
        if matches!(left, DataType::Null) || matches!(right, DataType::Null) {
            continue;
        }
        if type_category(left) != type_category(right) {
            return Err(CassieError::Planner(format!(
                "{} types {} and {} cannot be matched",
                operator_label(set.operator),
                left.type_name(),
                right.type_name()
            )));
        }
    }
    Ok(())
}

fn operand_column_types(select: &SelectStatement, catalog: &Catalog) -> Vec<Option<DataType>> {
    let QuerySource::Collection(collection) = &select.source else {
        return Vec::new();
    };
    if select
        .ctes
        .iter()
        .any(|cte| cte.name.eq_ignore_ascii_case(collection))
        || !select.group_by.is_empty()
    {
        return Vec::new();
    }
    let Some(schema) = catalog.get_schema(collection) else {
        return Vec::new();
    };
    let field_types: HashMap<String, DataType> = schema
        .fields
        .iter()
        .map(|field| (field.name.to_ascii_lowercase(), field.data_type.clone()))
        .collect();

    let mut types = Vec::with_capacity(select.projection.len());
    for item in &select.projection {
        match item {
            SelectItem::Wildcard => {
                let Ok(fields) =
                    wildcard_output_fields(&select.source, &[], catalog, &HashMap::new())
                else {
                    return Vec::new();
                };
                types.extend(fields.into_iter().map(|field| Some(field.data_type)));
            }
            SelectItem::Column { name, .. } => {
                types.push(crate::sql::field_type_for_column(&field_types, name).cloned());
            }
            SelectItem::Expr { expr, .. } => types.push(expr_type(expr, &field_types)),
            SelectItem::Function { .. } | SelectItem::WindowFunction { .. } => types.push(None),
        }
    }
    types
}

fn expr_type(expr: &Expr, field_types: &HashMap<String, DataType>) -> Option<DataType> {
    match expr {
        Expr::Column(name) => crate::sql::field_type_for_column(field_types, name).cloned(),
        Expr::Cast { data_type, .. } => Some(data_type.clone()),
        Expr::IntegerLiteral(_) => Some(DataType::Int),
        Expr::NumberLiteral(_) => Some(DataType::Float),
        Expr::BoolLiteral(_) => Some(DataType::Boolean),
        _ => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum TypeCategory {
    Numeric,
    Text,
    Boolean,
    DateTime,
    Exact(String),
}

fn type_category(data_type: &DataType) -> TypeCategory {
    match data_type {
        DataType::SmallInt | DataType::Int | DataType::BigInt | DataType::Float => {
            TypeCategory::Numeric
        }
        DataType::Text | DataType::Char { .. } | DataType::Varchar { .. } => TypeCategory::Text,
        DataType::Boolean => TypeCategory::Boolean,
        DataType::Date | DataType::Timestamp => TypeCategory::DateTime,
        other => TypeCategory::Exact(other.type_name()),
    }
}

const fn operator_label(operator: SetOperator) -> &'static str {
    match operator {
        SetOperator::Union | SetOperator::UnionAll => "UNION",
        SetOperator::Intersect => "INTERSECT",
        SetOperator::Except => "EXCEPT",
    }
}
