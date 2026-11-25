use super::super::capability::{self, Capability};
use super::super::scalar::Environment;
use super::super::TypedBatch;
use crate::sql::ast::{BinaryOp, Expr};
use crate::types::{DataType, Value};

fn null_safe(left: Expr, op: BinaryOp, right: Expr) -> Expr {
    Expr::Binary {
        left: Box::new(left),
        op,
        right: Box::new(right),
    }
}

fn numeric_batch(controls: &crate::runtime::QueryExecutionControls) -> TypedBatch {
    TypedBatch::from_columns(
        controls,
        &[
            ("i".to_owned(), DataType::BigInt),
            ("f".to_owned(), DataType::Float),
        ],
        &[
            vec![
                Value::Null,
                Value::Int64(7),
                Value::Null,
                Value::Int64(0),
                Value::Int64(9_007_199_254_740_993),
                Value::Int64(3),
            ],
            vec![
                Value::Null,
                Value::Null,
                Value::Float64(7.0),
                Value::Float64(-0.0),
                Value::Float64(9_007_199_254_740_992.0),
                Value::Float64(f64::NAN),
            ],
        ],
        6,
        Some(&[5, 4, 3, 2, 1, 0, 1]),
    )
    .expect("selected numeric source")
}

#[test]
fn should_admit_numeric_null_safe_comparisons_to_the_native_kernel() {
    // Arrange
    let controls = super::controls();
    let batch = numeric_batch(&controls);
    let params = [Value::Null, Value::Float64(7.0)];
    let operands = [
        (Expr::Column("i".to_owned()), Expr::Column("f".to_owned())),
        (Expr::Column("f".to_owned()), Expr::Column("i".to_owned())),
        (Expr::Column("i".to_owned()), Expr::Param(0)),
        (Expr::Param(1), Expr::Column("i".to_owned())),
        (Expr::Column("f".to_owned()), Expr::Null),
        (Expr::Null, Expr::Null),
        (Expr::Column("i".to_owned()), Expr::IntegerLiteral(7)),
        (Expr::Column("f".to_owned()), Expr::NumberLiteral(0.0)),
    ];
    for op in [BinaryOp::IsDistinctFrom, BinaryOp::IsNotDistinctFrom] {
        for (left, right) in operands.clone() {
            let expr = null_safe(left, op.clone(), right);
            // Act
            let capability = capability::expression(&expr, batch.schema(), &params);
            // Assert
            assert_eq!(capability, Capability::NativeTyped);
        }
    }
}

#[test]
fn should_match_scalar_null_safe_results_in_the_native_kernel() {
    // Arrange
    let controls = super::controls();
    let batch = numeric_batch(&controls);
    let functions = std::collections::HashMap::new();
    let params = [Value::Null, Value::Float64(7.0)];
    let env = Environment {
        controls: &controls,
        params: &params,
        functions: &functions,
        session: None,
    };
    let operands = [
        (Expr::Column("i".to_owned()), Expr::Column("f".to_owned())),
        (Expr::Column("f".to_owned()), Expr::Column("i".to_owned())),
        (Expr::Column("i".to_owned()), Expr::Param(0)),
        (Expr::Param(1), Expr::Column("i".to_owned())),
        (Expr::Column("f".to_owned()), Expr::Null),
        (Expr::Null, Expr::Null),
        (Expr::Column("f".to_owned()), Expr::NumberLiteral(0.0)),
    ];
    for op in [BinaryOp::IsDistinctFrom, BinaryOp::IsNotDistinctFrom] {
        for (left, right) in operands.clone() {
            let expr = null_safe(left, op.clone(), right);
            for lane in 0..batch.len() {
                // Act
                let scalar = env.evaluate(&batch, lane, &expr).expect("scalar null-safe");
                let native = super::super::operations::native_value(&batch, lane, &expr, &params)
                    .expect("native null-safe")
                    .to_owned();
                // Assert
                assert!(
                    matches!(native, Value::Bool(_)),
                    "null-safe result is never NULL"
                );
                assert_eq!(&native, scalar.get());
            }
        }
    }
}

#[test]
fn should_keep_nonnumeric_null_safe_operands_on_the_scalar_kernel() {
    // Arrange
    let controls = super::controls();
    let batch = TypedBatch::from_columns(
        &controls,
        &[
            ("s".to_owned(), DataType::SmallInt),
            ("flag".to_owned(), DataType::Boolean),
        ],
        &[
            vec![Value::Int64(1), Value::Null],
            vec![Value::Bool(true), Value::Null],
        ],
        2,
        None,
    )
    .expect("small source");
    let params = [Value::Bool(true), Value::String("1".to_owned())];
    let declined = [
        (Expr::Column("s".to_owned()), Expr::Param(0)),
        (Expr::Column("s".to_owned()), Expr::Param(1)),
        (Expr::Column("flag".to_owned()), Expr::BoolLiteral(true)),
    ];
    let admitted = (Expr::Column("s".to_owned()), Expr::NumberLiteral(1.5));
    // Act
    let declined = declined.map(|(left, right)| {
        capability::expression(
            &null_safe(left, BinaryOp::IsDistinctFrom, right),
            batch.schema(),
            &params,
        )
    });
    let admitted = null_safe(admitted.0, BinaryOp::IsNotDistinctFrom, admitted.1);
    // Assert
    assert!(declined
        .iter()
        .all(|capability| *capability == Capability::BoundedScalarExpression));
    assert_eq!(
        capability::expression(&admitted, batch.schema(), &params),
        Capability::NativeTyped
    );
    for lane in 0..batch.len() {
        let native = super::super::operations::native_value(&batch, lane, &admitted, &params)
            .expect("native smallint/float")
            .to_owned();
        assert_eq!(native, Value::Bool(false));
    }
}

#[test]
fn should_filter_composed_null_safe_predicates_natively() {
    // Arrange
    let controls = super::controls();
    let batch = numeric_batch(&controls);
    let functions = std::collections::HashMap::new();
    let predicate = null_safe(
        null_safe(
            Expr::Column("i".to_owned()),
            BinaryOp::IsNotDistinctFrom,
            Expr::Column("f".to_owned()),
        ),
        BinaryOp::Or,
        null_safe(
            Expr::Column("i".to_owned()),
            BinaryOp::IsNotDistinctFrom,
            Expr::IntegerLiteral(7),
        ),
    );
    assert_eq!(
        capability::expression(&predicate, batch.schema(), &[]),
        Capability::NativeTyped
    );

    // Act
    let filtered = batch
        .filter(&controls, &predicate, &[], &functions, None)
        .expect("native null-safe filter");

    // Assert
    let selected = (0..filtered.len())
        .map(|lane| {
            (
                filtered.value(0, lane).expect("integer lane"),
                filtered.value(1, lane).expect("float lane"),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        selected,
        vec![
            (Value::Int64(0), Value::Float64(-0.0)),
            (Value::Int64(7), Value::Null),
            (Value::Null, Value::Null),
            (Value::Int64(7), Value::Null),
        ]
    );
}
