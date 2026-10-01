use super::{CassieError, DataType, Expr, FunctionCall};

pub(super) fn validate_vector_function_query(
    function: &FunctionCall,
    field_types: &crate::sql::FieldTypeMap,
) -> Result<(), CassieError> {
    if !function.name.eq_ignore_ascii_case("vector_distance")
        && !function.name.eq_ignore_ascii_case("vector_score")
    {
        return Ok(());
    }
    let [field, query] = function.args.as_slice() else {
        return Ok(());
    };
    validate_vector_query_literals(field, query, field_types)
}

pub(super) fn validate_vector_query_literals(
    left: &Expr,
    right: &Expr,
    field_types: &crate::sql::FieldTypeMap,
) -> Result<(), CassieError> {
    let ((Expr::Column(field), Expr::StringLiteral(literal))
    | (Expr::StringLiteral(literal), Expr::Column(field))) = (left, right)
    else {
        return Ok(());
    };
    let Some(DataType::Vector(expected_dimensions)) =
        crate::sql::field_type_for_column(field_types, field)
    else {
        return Ok(());
    };
    let values = serde_json::from_str::<Vec<f64>>(literal)
        .map_err(|_| CassieError::Planner(format!("invalid vector query literal '{literal}'")))?;
    if values.is_empty() {
        return Err(CassieError::Planner(
            "vector distance query cannot be empty".to_string(),
        ));
    }
    if values
        .iter()
        .any(|value| crate::vector::f64_to_finite_f32(*value).is_none())
    {
        return Err(CassieError::Planner(
            "vector distance query component is outside f32 range".to_string(),
        ));
    }
    if values.len() != *expected_dimensions {
        return Err(CassieError::Planner(format!(
            "vector distance query for field '{field}' expects {expected_dimensions} dimensions but received {}",
            values.len()
        )));
    }
    Ok(())
}
