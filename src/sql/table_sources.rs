//! Selected built-in table-source identities and their physical column shapes.

use crate::app::CassieError;
use crate::sql::ast::{Expr, FunctionCall};
use crate::types::DataType;

pub(crate) fn is_unnest(name: &str) -> bool {
    name.eq_ignore_ascii_case("unnest") || name.eq_ignore_ascii_case("pg_catalog.unnest")
}
pub(crate) fn is_series(name: &str) -> bool {
    name.eq_ignore_ascii_case("generate_series")
        || name.eq_ignore_ascii_case("pg_catalog.generate_series")
}
pub(crate) fn is_generated(name: &str) -> bool {
    is_unnest(name) || is_series(name)
}

pub(crate) fn physical_name(index: usize) -> String {
    format!("column{}", index + 1)
}

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
