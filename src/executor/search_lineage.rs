//! Owned comparison keys for the existing generated SEARCH record carrier.
use std::mem::size_of;

use super::batch::RowAccess;
use super::lineage_keys::{self, KeyType};
use super::retained_memory::{add, mul};
use super::semantic::{RecordKey, SemanticValue};
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::sql::ast::{CteSearchOrder, Expr};
use crate::types::{typed::TypedKey, Value};

const PREFIX: &str = "\0cassie_cte_lineage:";
#[derive(serde::Deserialize)]
struct Lineage {
    search_path: Vec<Vec<Value>>,
    #[serde(default, rename = "search_column")]
    column: Option<String>,
    #[serde(default, rename = "search_order")]
    order: Option<CteSearchOrder>,
    #[serde(default)]
    search_key_types: Vec<KeyType>,
}

pub(super) fn extract<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    controls: Option<&QueryExecutionControls>,
) -> Result<Option<SemanticValue>, CassieError> {
    extract_observing(row, expr, controls, || {})
}
fn extract_observing<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    controls: Option<&QueryExecutionControls>,
    mut after_normalize: impl FnMut(),
) -> Result<Option<SemanticValue>, CassieError> {
    let Expr::Column(name) = expr else {
        return Ok(None);
    };
    if !row
        .entries()
        .iter()
        .any(|(name, _)| name.starts_with(PREFIX))
    {
        return Ok(None);
    }
    let _identifier = reserve(controls, add(mul(name.len(), 32)?, 128)?)?;
    let Some(column) = crate::sql::ColumnIdentifierPath::parse(name)
        .ok()
        .map(|path| path.final_name().to_owned())
    else {
        return Ok(None);
    };
    for (_, value) in row
        .entries()
        .iter()
        .filter(|(name, _)| name.starts_with(PREFIX))
    {
        let Value::String(json) = value else {
            continue;
        };
        let workspace = mul(
            crate::runtime::accounted::json::serialized_decode_bytes(json.len())?,
            3,
        )?;
        let key = RecordKey::build(controls, workspace, || {
            let lineage: Lineage = serde_json::from_str(json).map_err(|error| {
                CassieError::Execution(format!("invalid private SEARCH lineage: {error}"))
            })?;
            if lineage.column.as_deref() != Some(column.as_str()) {
                return Ok(None);
            }
            let Some(order) = lineage.order else {
                return Ok(None);
            };
            let (value, retained) = normalize(&lineage, order, controls)?;
            after_normalize();
            Ok(Some((value, retained)))
        })?;
        if let Some(key) = key {
            return Ok(Some(SemanticValue::Record(key)));
        }
    }
    Ok(None)
}
fn normalize(
    lineage: &Lineage,
    order: CteSearchOrder,
    controls: Option<&QueryExecutionControls>,
) -> Result<(SemanticValue, usize), CassieError> {
    if lineage.search_path.is_empty() {
        return Err(CassieError::Execution(
            "private SEARCH path is empty".into(),
        ));
    }
    match order {
        CteSearchOrder::DepthFirst => {
            let mut values = Vec::with_capacity(lineage.search_path.len());
            let mut retained = mul(values.capacity(), size_of::<SemanticValue>())?;
            for source in &lineage.search_path {
                let (tuple, bytes) = tuple(source, &lineage.search_key_types, controls)?;
                retained = add(retained, bytes)?;
                values.push(SemanticValue::Array(tuple));
            }
            Ok((SemanticValue::Array(values), retained))
        }
        CteSearchOrder::BreadthFirst => {
            let Some(source) = lineage.search_path.last() else {
                return Err(CassieError::Execution(
                    "private SEARCH path is empty".into(),
                ));
            };
            let (tuple, bytes) = tuple(source, &lineage.search_key_types, controls)?;
            let mut fields = Vec::with_capacity(tuple.len() + 1);
            let retained = add(
                mul(fields.capacity(), size_of::<SemanticValue>())?,
                bytes.saturating_sub(tuple.capacity() * size_of::<SemanticValue>()),
            )?;
            let depth =
                i64::try_from(lineage.search_path.len().saturating_sub(1)).unwrap_or(i64::MAX);
            fields.push(SemanticValue::from_value(&Value::Int64(depth)));
            fields.extend(tuple);
            Ok((SemanticValue::Array(fields), retained))
        }
    }
}
fn tuple(
    source: &[Value],
    types: &[KeyType],
    controls: Option<&QueryExecutionControls>,
) -> Result<(Vec<SemanticValue>, usize), CassieError> {
    lineage_keys::validate_width(types, source.len())?;
    let mut values = Vec::with_capacity(source.len());
    let mut retained = mul(values.capacity(), size_of::<SemanticValue>())?;
    for (index, value) in source.iter().enumerate() {
        let key = lineage_keys::key_type(types, index).key(value, controls)?;
        retained = add(retained, heap(&key, value)?)?;
        values.push(key);
    }
    Ok((values, retained))
}
fn heap(key: &SemanticValue, source: &Value) -> Result<usize, CassieError> {
    match (key, source) {
        (
            SemanticValue::Typed(TypedKey::Numeric(_)),
            Value::Typed(crate::types::typed::TypedValue::Numeric(
                crate::types::extended::numeric::Numeric::Finite { mantissa, .. },
            )),
        ) => {
            // order_key only divides/clones the source limbs; normalization can keep capacity.
            add(
                mul(
                    usize::try_from(mantissa.bits().div_ceil(8)).map_err(|_| overflow())?,
                    2,
                )?,
                64,
            )
        }
        (SemanticValue::Typed(TypedKey::Bits(bytes, _)), _) => Ok(bytes.capacity()),
        (
            SemanticValue::SqlArray {
                dimensions,
                elements,
            },
            Value::Array(array),
        ) => elements.iter().zip(array.elements()).try_fold(
            add(
                mul(dimensions.capacity(), size_of::<(i32, i32)>())?,
                mul(elements.capacity(), size_of::<SemanticValue>())?,
            )?,
            |bytes, (key, source)| add(bytes, heap(key, source)?),
        ),
        _ => super::sort::accounting::semantic_heap(key),
    }
}
fn reserve(
    controls: Option<&QueryExecutionControls>,
    bytes: usize,
) -> Result<Option<QueryMemoryReservation>, CassieError> {
    controls
        .map(|controls| controls.reserve_query_memory(bytes))
        .transpose()
}
fn overflow() -> CassieError {
    CassieError::ResourceLimit("private SEARCH key accounting overflow".into())
}

#[cfg(test)]
#[path = "search_lineage_tests.rs"]
mod tests;
