use super::super::{capability, operations, scalar::Environment, TypedBatch};
use crate::sql::ast::{Expr, TruthValue};
use crate::types::{DataType, Value};

#[test]
fn should_match_native_truth_tables_given_selected_boolean_lanes_when_kernels_are_compared() {
    // Arrange
    let controls = super::controls();
    let batch = TypedBatch::from_columns(
        &controls,
        &[("flag".into(), DataType::Boolean)],
        &[vec![Value::Bool(true), Value::Bool(false), Value::Null]],
        3,
        Some(&[2, 0, 2, 1]),
    )
    .expect("selected Boolean batch");
    let functions = std::collections::HashMap::new();
    let env = Environment {
        controls: &controls,
        params: &[],
        functions: &functions,
        session: None,
    };
    let cases = [
        (TruthValue::True, [false, true, false, false]),
        (TruthValue::False, [false, false, false, true]),
        (TruthValue::Unknown, [true, false, true, false]),
    ];
    for (truth, expected) in cases {
        for negated in [false, true] {
            let expr = Expr::TruthTest {
                expr: Box::new(Expr::Column("flag".into())),
                truth,
                negated,
            };
            // Act
            let native = (0..batch.len())
                .map(|lane| {
                    operations::native_value(&batch, lane, &expr, &[])
                        .expect("native truth")
                        .to_owned()
                })
                .collect::<Vec<_>>();
            let scalar = (0..batch.len())
                .map(|lane| {
                    env.evaluate(&batch, lane, &expr)
                        .expect("scalar truth")
                        .get()
                        .clone()
                })
                .collect::<Vec<_>>();
            // Assert
            assert_eq!(
                capability::expression(&expr, batch.schema(), &[]),
                capability::Capability::NativeTyped
            );
            assert_eq!(native, expected.map(|value| Value::Bool(value != negated)));
            assert_eq!(native, scalar);
        }
    }
}
