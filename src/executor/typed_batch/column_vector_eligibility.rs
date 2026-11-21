//! When a column-authoritative scan reads vector batches (#1358).
//!
//! One decision serves both the scan dispatch and EXPLAIN, so the plan text and
//! the executed path cannot disagree. A scan reads vector batches when the
//! table is column-authoritative, every field it needs is a plain stored field
//! of a supported scalar type, and the typed operator above it accepts the plan
//! shape. Everything else reads row documents and reports why.
//!
//! Supported types decode straight into lanes: `SMALLINT`, `INT`, `BIGINT`,
//! `FLOAT`, `BOOLEAN`, `TEXT`, `VARCHAR`, `CHAR` (native lanes) and `NAME`,
//! `UUID`, `DATE`, `TIME`, `TIMESTAMP` and the extended scalar types (generic
//! value lanes). Document-shaped types (JSON, arrays, composites, domains,
//! enums, vectors, bytea) keep the row path.

use crate::types::DataType;

/// The outcome of the shared eligibility decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ColumnVectorEligibility {
    /// The table is not column-authoritative.
    NotApplicable,
    /// The scan reads vector batches.
    Eligible,
    /// The scan reads row documents for the labelled reason.
    Fallback(String),
}

impl ColumnVectorEligibility {
    /// The `column_vector=` EXPLAIN token.
    pub(crate) fn token(&self) -> &'static str {
        match self {
            Self::NotApplicable => "not_applicable",
            Self::Eligible => "eligible",
            Self::Fallback(_) => "fallback",
        }
    }

    /// The `column_vector_reason=` EXPLAIN token.
    pub(crate) fn reason(&self) -> &str {
        match self {
            Self::NotApplicable => "not-column-authoritative",
            Self::Eligible => "none",
            Self::Fallback(reason) => reason,
        }
    }

    pub(crate) fn unsupported_type(data_type: &DataType) -> Self {
        Self::Fallback(format!(
            "column-vector-unsupported-type:{}",
            data_type.type_name()
        ))
    }

    pub(crate) fn expression() -> Self {
        Self::Fallback("column-vector-unsupported-expression".to_owned())
    }

    pub(crate) fn aggregate_shape() -> Self {
        Self::Fallback("column-vector-unsupported-aggregate-shape".to_owned())
    }

    pub(crate) fn transport_budget() -> Self {
        Self::Fallback("column-vector-unsupported-transport-budget".to_owned())
    }

    pub(crate) fn join() -> Self {
        Self::Fallback("column-vector-unsupported-join".to_owned())
    }

    pub(crate) fn covering_index() -> Self {
        Self::Fallback("column-vector-covering-column-index".to_owned())
    }
}

/// Whether a stored field of this type decodes into vector lanes.
pub(crate) fn supports_type(data_type: &DataType) -> bool {
    matches!(
        data_type,
        DataType::SmallInt
            | DataType::Int
            | DataType::BigInt
            | DataType::Float
            | DataType::Boolean
            | DataType::Text
            | DataType::Varchar { .. }
            | DataType::Char { .. }
            | DataType::Name
            | DataType::Uuid
            | DataType::Date
            | DataType::Time
            | DataType::Timestamp
            | DataType::Extended(_)
    )
}

/// The first unsupported type among `types`, as an eligibility fallback.
pub(crate) fn type_eligibility<'a>(
    types: impl IntoIterator<Item = &'a DataType>,
) -> ColumnVectorEligibility {
    types
        .into_iter()
        .find(|data_type| !supports_type(data_type))
        .map_or(ColumnVectorEligibility::Eligible, |data_type| {
            ColumnVectorEligibility::unsupported_type(data_type)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_label_the_first_unsupported_type_in_the_fallback_reason() {
        // Arrange
        let types = [DataType::BigInt, DataType::Jsonb, DataType::Bytea];

        // Act
        let eligibility = type_eligibility(&types);

        // Assert
        assert_eq!(eligibility.token(), "fallback");
        assert_eq!(eligibility.reason(), "column-vector-unsupported-type:jsonb");
    }

    #[test]
    fn should_accept_scalar_types_without_a_reason() {
        // Arrange
        let types = [
            DataType::Int,
            DataType::Float,
            DataType::Text,
            DataType::Date,
        ];

        // Act
        let eligibility = type_eligibility(&types);

        // Assert
        assert_eq!(eligibility, ColumnVectorEligibility::Eligible);
        assert_eq!(eligibility.reason(), "none");
    }
}
