//! Engine adapter for portable SQL table-source declarations.

pub(crate) use cassie_sql::registry::table_sources::{
    is_generated, is_graph_table, is_json_table, is_partition_ancestors, is_query_column,
    is_series, is_unnest, json_element_columns, json_element_source, json_member_name,
    physical_name, record_declared_columns, record_output_columns, record_source,
    JsonElementSource, RecordSource,
};

use crate::app::CassieError;
use crate::sql::ast::{Expr, FunctionCall};
use crate::types::DataType;

/// The executor supplies selected argument types because resolution depends on the engine's
/// binding context. SQL source identities and their static shapes live in `cassie-sql`.
pub(crate) fn unnest_columns(
    function: &FunctionCall,
    with_ordinality: bool,
    mut argument_type: impl FnMut(&Expr) -> Option<DataType>,
) -> Result<Vec<(String, DataType)>, CassieError> {
    let mut columns = Vec::with_capacity(function.args.len() + usize::from(with_ordinality));
    for (index, argument) in function.args.iter().enumerate() {
        let kind = match argument_type(argument) {
            Some(DataType::Array(element)) => scalar_element(*element),
            Some(DataType::Null) | None => DataType::Null,
            Some(_) => {
                return Err(CassieError::Execution(
                    "function unnest(non-array) does not exist".into(),
                ));
            }
        };
        columns.push((physical_name(index), kind));
    }
    if with_ordinality {
        columns.push((physical_name(function.args.len()), DataType::BigInt));
    }
    Ok(columns)
}

fn scalar_element(mut kind: DataType) -> DataType {
    while let DataType::Array(element) = kind {
        kind = *element;
    }
    // An ordinary set-returning FuncExpr has typmod -1 even when the
    // incoming array's elements were explicitly constrained. Values already
    // carry that input coercion; only the scalar result descriptor is erased.
    i32::try_from(kind.type_oid())
        .ok()
        .and_then(super::binder::parameter_data_type_for_oid)
        .unwrap_or(kind)
}

#[cfg(test)]
mod tests {
    use super::json_member_name;

    #[test]
    fn should_unquote_a_delimited_lookup_key_given_a_declared_column() {
        // Arrange
        let declared = "\"user\"\"Id\"";

        // Act
        let key = json_member_name(declared);

        // Assert
        assert_eq!(key, "user\"Id");
    }

    #[test]
    fn should_keep_a_plain_lookup_key_given_a_declared_column() {
        // Arrange
        let declared = "userid";

        // Act
        let key = json_member_name(declared);

        // Assert
        assert_eq!(key, "userid");
    }
}
