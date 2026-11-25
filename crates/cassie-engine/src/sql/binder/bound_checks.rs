//! Prepare the existing simple CHECK shape with the ordinary expression binder.
use super::{
    boolean_contexts, coalesce_results::ResultTypes, BindingContext, CassieError, Catalog,
};
use crate::catalog::{ConstraintOperator, FieldConstraint};
use crate::sql::ast::{BinaryOp, CastIdentity, Expr, QuerySource};
use crate::types::{DataType, FieldSchema};

pub(super) fn prepare(
    constraints: &mut [FieldConstraint],
    fields: &[FieldSchema],
) -> Result<(), CassieError> {
    let catalog = Catalog::default();
    let context = BindingContext::default();
    for check in constraints
        .iter_mut()
        .filter_map(|constraint| constraint.check.as_mut())
    {
        let op = match check.operator {
            ConstraintOperator::IsDistinctFrom => BinaryOp::IsDistinctFrom,
            ConstraintOperator::IsNotDistinctFrom => BinaryOp::IsNotDistinctFrom,
            _ => continue,
        };
        if check.value.is_null() {
            continue;
        }
        let field = fields
            .iter()
            .find(|field| {
                crate::sql::ColumnIdentifierPath::matches_stored_field(&check.field, &field.name)
            })
            .ok_or_else(|| {
                CassieError::Planner(format!("unknown CHECK field '{}'", check.field))
            })?;
        let types = ResultTypes::for_source(&QuerySource::SingleRow, &[], &catalog, &context)?
            .with_extra_fields(vec![FieldSchema {
                name: check.field.clone(),
                data_type: field.data_type.clone(),
                nullable: true,
            }])
            .with_resolved_coalesce_domains(true);
        let target = match &field.data_type {
            DataType::Char { .. } => DataType::Char { length: None },
            DataType::Varchar { .. } => DataType::Varchar { length: None },
            other => other.clone(),
        };
        let mut expression = Expr::Binary {
            left: Box::new(Expr::Cast {
                expr: Box::new(Expr::Column(check.field.clone())),
                data_type: target,
                source_type: Some(field.data_type.clone()),
                identity: CastIdentity::Scalar,
            }),
            op,
            right: Box::new(literal(&check.value)?),
        };
        boolean_contexts::validate_predicate(&mut expression, &types, "CHECK", &catalog, &context)?;
        let field_types =
            std::collections::HashMap::from([(check.field.clone(), field.data_type.clone())]);
        super::validate_expression_operand_families(&expression, &field_types)?;
        let Expr::Binary { right, .. } = &expression else {
            return Err(CassieError::Unsupported(
                "CHECK binding must retain its simple comparison".into(),
            ));
        };
        if matches!(
            field.data_type,
            DataType::SmallInt
                | DataType::Int
                | DataType::BigInt
                | DataType::Float
                | DataType::Boolean
                | DataType::Text
        ) {
            crate::executor::filter::evaluate_expr_value(
                &Vec::<(String, crate::types::Value)>::new(),
                right,
                &[],
                None,
                &std::collections::HashMap::new(),
                None,
                None,
            )
            .map_err(CassieError::from)?;
        }
        check.value = crate::sql::check_comparison::literal(right)
            .ok_or_else(|| CassieError::Unsupported("CHECK must retain a bound literal".into()))?;
        check.bound_expression = Some(Box::new(expression));
        crate::sql::check_comparison::validate(check)?;
    }
    Ok(())
}

fn literal(value: &serde_json::Value) -> Result<Expr, CassieError> {
    Ok(match value {
        serde_json::Value::Null => Expr::Null,
        serde_json::Value::Bool(value) => Expr::BoolLiteral(*value),
        serde_json::Value::String(value) => Expr::StringLiteral(value.clone()),
        serde_json::Value::Number(value) => {
            crate::sql::parser::parse_expression(&value.to_string())
                .map_err(|error| CassieError::InvalidQuery(error.to_string()))?
        }
        _ => {
            return Err(CassieError::Unsupported(
                "CHECK requires a simple scalar literal".into(),
            ))
        }
    })
}
