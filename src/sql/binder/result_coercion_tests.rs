use std::cell::Cell;
use std::collections::HashMap;

use super::{float, original_operand, DataType, Expr};
use crate::executor::batch::RowAccess;
use crate::executor::filter::evaluate_expr_value;
use crate::types::Value;

struct ReadOnceRow {
    entries: [(String, Value); 1],
    reads: Cell<usize>,
}

impl RowAccess for ReadOnceRow {
    fn get(&self, name: &str) -> Option<&Value> {
        self.reads.set(self.reads.get() + 1);
        self.entries
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    fn entries(&self) -> &[(String, Value)] {
        &self.entries
    }
}

fn assert_carrier(actual: &Value, expected: &Value) {
    if let (Value::Float64(actual), Value::Float64(expected)) = (actual, expected) {
        assert_eq!(actual.to_bits(), expected.to_bits());
    } else {
        assert_eq!(actual, expected);
    }
}

#[test]
fn should_preserve_generated_float_wrapper_evaluation() {
    // Arrange
    let operand = Expr::Column("n".into());
    let generated = float(operand.clone());
    let manual = Expr::Cast {
        expr: Box::new(Expr::Case {
            operand: None,
            branches: Vec::new(),
            else_expr: Some(Box::new(operand)),
        }),
        data_type: DataType::Float,
    };
    let repeated = float(generated.clone());
    let cases = [
        (
            Value::Int64(9_007_199_254_740_993),
            Some(Value::Float64(9_007_199_254_740_992.0)),
        ),
        (Value::Float64(-0.0), Some(Value::Float64(-0.0))),
        (
            Value::Float64(f64::INFINITY),
            Some(Value::Float64(f64::INFINITY)),
        ),
        (
            Value::Float64(f64::NEG_INFINITY),
            Some(Value::Float64(f64::NEG_INFINITY)),
        ),
        (
            Value::Float64(f64::from_bits(0xfff8_0000_0000_0001)),
            Some(Value::Float64(f64::from_bits(0xfff8_0000_0000_0001))),
        ),
        (Value::Null, Some(Value::Null)),
        (Value::String("not-a-number".into()), None),
    ];
    let functions = HashMap::new();

    // Act
    let observations = cases
        .into_iter()
        .flat_map(|(value, expected)| {
            [&generated, &manual, &repeated].map(|expression| {
                let row = ReadOnceRow {
                    entries: [("n".into(), value.clone())],
                    reads: Cell::new(0),
                };
                let result =
                    evaluate_expr_value(&row, expression, &[], None, &functions, None, None);
                (result, expected.clone(), row.reads.get())
            })
        })
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(
        serde_json::to_value(&generated).expect("generated AST"),
        serde_json::to_value(&manual).expect("manual AST")
    );
    assert_eq!(
        serde_json::to_value(&generated).expect("generated AST"),
        serde_json::to_value(&repeated).expect("idempotent AST")
    );
    assert!(matches!(original_operand(&generated), Some(Expr::Column(name)) if name == "n"));
    for (result, expected, reads) in observations {
        assert_eq!(reads, 1, "the original operand is evaluated once");
        if let Some(expected) = expected {
            assert_carrier(&result.expect("admitted numeric carrier"), &expected);
        } else {
            assert!(result.is_err(), "nonnumeric coercion remains an error");
        }
    }
}
