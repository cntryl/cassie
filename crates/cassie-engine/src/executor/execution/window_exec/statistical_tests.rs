//! Scalar statistical window output keeps its native result owner after folding.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::types::extended::numeric::parse_text;
use crate::types::typed::TypedValue;
use std::time::Instant;

#[test]
fn should_keep_numeric_window_outputs_owned_until_the_returned_rows_are_released() {
    // Arrange
    let parsed = crate::sql::parse_statement("SELECT var_pop(n) FILTER(WHERE TRUE) OVER ()")
        .expect("statistical window");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT");
    };
    let rows = ["1.125", "3.125"].map(|text| {
        BatchRow::new(vec![(
            "n".into(),
            Value::Typed(TypedValue::Numeric(parse_text(text).expect("numeric"))),
        )])
    });
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let functions = HashMap::new();

    // Act
    let output = apply_window_functions(
        vec![rows.into()],
        &select.projection,
        &[],
        None,
        &functions,
        None,
        &controls,
    )
    .expect("window output");
    let retained = controls.current_query_memory_bytes();

    // Assert
    assert!(retained > 0);
    assert!(matches!(
        output[0][0].entries().last().expect("window value").1,
        Value::Typed(TypedValue::Numeric(_))
    ));
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
