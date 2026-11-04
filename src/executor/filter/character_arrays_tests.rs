use super::*;
use crate::runtime::QueryCancellationHandle;
use std::time::Instant;

#[test]
fn should_admit_all_character_array_padding_before_converting_any_element() {
    // Arrange
    let array = SqlArray::from_elements(vec![
        Value::String("é".into()),
        Value::Null,
        Value::String("x".into()),
    ])
    .expect("native array");
    let element = DataType::Char { length: Some(32) };
    let target = DataType::Array(Box::new(element.clone()));
    let required = scratch_bytes(&array, &element, None).expect("padding bound");

    // Act / Assert
    for (budget, accepted) in [(required - 1, false), (required, true)] {
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits {
                query_memory_budget_bytes: budget,
                ..crate::config::CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let memory = EvaluationMemory::new(&controls).expect("expression owner");
        CONVERTED_ELEMENTS.with(|count| count.set(0));
        let result = cast(
            &ScalarValue::Array(array.clone()),
            &target,
            Some(&DataType::Array(Box::new(DataType::Text))),
            Some(&controls),
            Some(&memory),
            true,
        )
        .expect("character array cast");
        assert_eq!(result.is_ok(), accepted);
        assert_eq!(
            CONVERTED_ELEMENTS.with(std::cell::Cell::get),
            if accepted { 2 } else { 0 }
        );
        if accepted {
            assert_eq!(controls.current_query_memory_bytes(), required);
        } else {
            assert!(matches!(
                result,
                Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
            ));
        }
        drop(result);
        drop(memory);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_character_array_scratch_when_cancelled_between_element_conversions() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let memory = EvaluationMemory::new(&controls).expect("expression owner");
    let array = SqlArray::from_elements(vec![Value::String("a".into()), Value::String("b".into())])
        .expect("native array");
    let target = DataType::Array(Box::new(DataType::Char { length: Some(32) }));
    CONVERTED_ELEMENTS.with(|count| count.set(0));
    CANCEL_AFTER_ELEMENT.with(|slot| *slot.borrow_mut() = Some(cancellation));

    // Act
    let result = cast(
        &ScalarValue::Array(array),
        &target,
        None,
        Some(&controls),
        Some(&memory),
        true,
    )
    .expect("character array cast");
    let converted = CONVERTED_ELEMENTS.with(std::cell::Cell::get);
    drop(memory);

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::QueryCancelled))
    ));
    assert_eq!(converted, 1);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_validate_large_character_array_input_without_allocating_its_padding() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 4096,
            ..crate::config::CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let target = DataType::Array(Box::new(DataType::Char {
        length: Some(1_000_000),
    }));

    // Act
    let result = validate_input("{x}", &target, Some(&controls)).expect("character array input");

    // Assert
    result.expect("validation does not materialize padding");
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_character_array_constant_padding_until_its_returned_owner_is_released() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
    );
    let expression = crate::sql::parser::parse_expression("$1::CHAR(3)[]").expect("array cast");
    let value = Value::Array(
        SqlArray::from_elements(vec![Value::String("x".into()), Value::Null])
            .expect("native array"),
    );

    // Act
    let (result, memory) =
        crate::executor::filter::constant_scalar_with_parameters(&expression, &[value], &controls)
            .expect("native constant cast");
    let retained = controls.current_query_memory_bytes();

    // Assert
    let Some(ScalarValue::Array(array)) = result else {
        panic!("native character array");
    };
    assert_eq!(
        array.elements(),
        &[Value::String("x  ".into()), Value::Null]
    );
    assert!(retained > 0);
    drop(array);
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_native_numeric_formatting_before_truncating_character_array_elements() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 4096,
            ..crate::config::CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let memory = EvaluationMemory::new(&controls).expect("expression owner");
    let value = Value::Typed(crate::types::typed::TypedValue::Numeric(
        crate::types::extended::numeric::Numeric::finite(num_bigint::BigInt::from(1), 4096),
    ));
    let input = ScalarValue::Array(SqlArray::from_elements(vec![value]).expect("numeric array"));
    let target = DataType::Array(Box::new(DataType::Char { length: Some(3) }));
    CONVERTED_ELEMENTS.with(|count| count.set(0));

    // Act
    let result =
        cast(&input, &target, None, Some(&controls), Some(&memory), true).expect("character cast");
    let converted = CONVERTED_ELEMENTS.with(std::cell::Cell::get);
    drop(memory);

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
    ));
    assert_eq!(converted, 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
