//! pgvector distance operators over text-carried vectors.
use super::{parse_vector_text, BinaryOp, QueryError, ScalarValue};

pub(super) fn vector_distance(
    op: &BinaryOp,
    left: &ScalarValue,
    right: &ScalarValue,
) -> Result<f64, QueryError> {
    let left = left
        .as_str()
        .and_then(parse_vector_text)
        .ok_or_else(|| QueryError::General("invalid vector distance operand".to_string()))?;
    let right = right
        .as_str()
        .and_then(parse_vector_text)
        .ok_or_else(|| QueryError::General("invalid vector distance operand".to_string()))?;
    if left.is_empty() || right.is_empty() {
        return Err(QueryError::General(
            "vector distance operand cannot be empty".to_string(),
        ));
    }
    if left.len() != right.len() {
        return Err(QueryError::General(format!(
            "vector distance dimension mismatch: {} != {}",
            left.len(),
            right.len()
        )));
    }

    Ok(match op {
        BinaryOp::PgvectorCosine => crate::vector::cosine_distance(&left, &right),
        BinaryOp::PgvectorL2 => crate::vector::l2_distance(&left, &right),
        BinaryOp::PgvectorDot => -crate::vector::dot_score(&left, &right),
        _ => 0.0,
    })
}
