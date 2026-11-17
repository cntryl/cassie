//! Shared admitted semantic-key conversion for scalar SORT and WINDOW.
use std::collections::HashMap;

use crate::catalog::FunctionMeta;
use crate::executor::batch::RowAccess;
use crate::executor::semantic::SemanticValue;
use crate::executor::QueryError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::sql::ast::Expr;
use crate::types::Value;

use super::accounting;

struct PendingValue {
    value: Value,
    memory: QueryMemoryReservation,
}

pub(crate) fn convert<R: RowAccess>(
    row: &R,
    expression: &Expr,
    value: Value,
    functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
    before_key: impl FnOnce() -> Result<(), QueryError>,
) -> Result<(SemanticValue, QueryMemoryReservation), QueryError> {
    let scratch = accounting::type_scratch(row, expression, functions)?;
    let memory = controls.reserve_query_memory(scratch)?;
    let mut pending = PendingValue { value, memory };
    let data_type = crate::executor::array_order::expression_type(row, expression, functions);
    let mut admitted = accounting::conversion_bytes(&pending.value, data_type.as_ref())?;
    let mut vector_type = None;
    let mut numeric_type = None;
    let mut bit_type = None;
    if let Some(extra) = crate::executor::array_order::sort_vector::bound(
        &pending.value,
        data_type.as_ref(),
        controls,
    )? {
        admitted = crate::executor::retained_memory::add(admitted, extra)?;
        vector_type = data_type;
    } else if let Some(extra) =
        crate::executor::array_order::sort_numeric::bound(&pending.value, data_type.as_ref())?
    {
        admitted = crate::executor::retained_memory::add(admitted, extra)?;
        numeric_type = data_type;
    } else if crate::executor::array_order::sort_bit::eligible(&pending.value, data_type.as_ref()) {
        bit_type = data_type;
    }
    pending.memory.try_grow(admitted)?;
    before_key()?;
    let (semantic, known_retained) = if let Some(data_type) = vector_type.as_ref() {
        crate::executor::array_order::sort_vector_key(
            row,
            expression,
            &pending.value,
            functions,
            data_type,
            controls,
        )?
    } else if let Some(data_type) = numeric_type.as_ref() {
        crate::executor::array_order::sort_numeric_key(
            row,
            expression,
            &pending.value,
            functions,
            data_type,
            controls,
        )?
    } else if let Some(data_type) = bit_type.as_ref() {
        crate::executor::array_order::sort_bit_key(
            row,
            expression,
            &pending.value,
            functions,
            data_type,
            controls,
        )?
    } else {
        (
            crate::executor::array_order::key(
                row,
                expression,
                &pending.value,
                functions,
                Some(controls),
            )?,
            None,
        )
    };
    let retained = match known_retained {
        Some(retained) => retained,
        None => accounting::semantic_heap(&semantic)?,
    };
    if retained > admitted {
        return Err(crate::app::CassieError::ResourceLimit(
            "sort key exceeded its admitted allocation shape".into(),
        )
        .into());
    }
    let PendingValue { value, mut memory } = pending;
    drop(value);
    memory.shrink_to(retained);
    Ok((semantic, memory))
}

#[cfg(test)]
mod record_memory_tests {
    use super::*;
    use crate::executor::batch::BatchRow;
    use crate::types::composite::RecordValue;
    use crate::types::typed::TypedValue;
    use crate::types::{DataType, SqlArray};

    fn record_with_array() -> Value {
        let array = SqlArray::from_elements(vec![Value::String("short-key".to_owned())])
            .expect("tiny array");
        Value::Typed(TypedValue::Record(RecordValue::anonymous(vec![(
            Value::Array(array),
            DataType::Array(Box::new(DataType::Text)),
        )])))
    }

    fn minimum_heap() -> usize {
        // One outer field slot, one array element slot, lengths/lower-bound buffers and text.
        2 * std::mem::size_of::<usize>() + 2 * std::mem::size_of::<i32>() + "short-key".len()
    }

    fn controls() -> QueryExecutionControls {
        QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            std::time::Instant::now(),
        )
    }

    #[test]
    fn should_retain_native_record_array_key_memory_on_the_sort_owner_until_drop() {
        // Arrange
        let controls = controls();
        let row = BatchRow::new(Vec::new());
        let expression = Expr::Column("record_key".to_owned());
        let functions = HashMap::new();

        // Act
        let (key, lease) = convert(
            &row,
            &expression,
            record_with_array(),
            &functions,
            &controls,
            || Ok(()),
        )
        .expect("native record key conversion");
        let retained = controls.current_query_memory_bytes();
        let charged = lease.bytes_for_test();
        let same_owner = lease.belongs_to(&controls);
        drop(key);
        drop(lease);

        // Assert
        assert!(same_owner);
        assert!(
            charged >= minimum_heap(),
            "the key's own Vec/text allocations remain admitted"
        );
        assert_eq!(
            retained, charged,
            "there is no input row or unrelated owner in this fixture"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_include_native_record_array_backing_in_the_ordinary_semantic_estimate() {
        // Arrange
        let value = record_with_array();

        // Act
        let key = SemanticValue::from_value(&value);
        let estimated = key.estimated_bytes();

        // Assert
        assert!(
            estimated >= minimum_heap(),
            "record array backing is owned by this semantic value"
        );
    }

    #[test]
    fn should_include_native_record_array_backing_in_the_controlled_semantic_estimate() {
        // Arrange
        let controls = controls();
        let value = record_with_array();
        let key = crate::executor::semantic::SemanticKey::single(&value);

        // Act
        let estimated = key
            .estimated_bytes_controlled(&controls)
            .expect("controlled estimate");

        // Assert
        assert!(
            estimated >= std::mem::size_of::<SemanticValue>() + minimum_heap(),
            "the semantic directory and record array backing are distinct allocations"
        );
        assert_eq!(
            controls.current_query_memory_bytes(),
            0,
            "estimation retains no new owner"
        );
    }
}
