use std::collections::HashMap;

use super::super::capability::{self, Capability};
use super::super::TypedBatch;
use crate::executor::batch::BatchRow;
use crate::executor::filter::evaluate_expr_value;
use crate::sql::ast::{BinaryOp, Expr, SelectItem};
use crate::types::{DataType, Value};

#[test]
fn should_preserve_sliced_selection_through_native_filter_projection() {
    // Arrange
    let controls = super::controls();
    let schema = [
        ("n".to_owned(), DataType::BigInt),
        ("flag".to_owned(), DataType::Boolean),
        ("payload".to_owned(), DataType::Text),
    ];
    let columns = source_columns();
    let predicate = Expr::Binary {
        left: Box::new(Expr::Binary {
            left: Box::new(Expr::Column("n".to_owned())),
            op: BinaryOp::Gte,
            right: Box::new(Expr::IntegerLiteral(0)),
        }),
        op: BinaryOp::And,
        right: Box::new(Expr::Column("flag".to_owned())),
    };
    let projection =
        [("n", "x"), ("n", "y"), ("payload", "p"), ("payload", "q")].map(|(name, alias)| {
            SelectItem::Column {
                name: name.to_owned(),
                alias: Some(alias.to_owned()),
            }
        });
    let expected_schema = [
        ("x".to_owned(), DataType::BigInt),
        ("y".to_owned(), DataType::BigInt),
        ("p".to_owned(), DataType::Text),
        ("q".to_owned(), DataType::Text),
    ];
    let expected_rows = vec![
        vec![
            Value::Int64(40),
            Value::Int64(40),
            Value::String("λ".to_owned()),
            Value::String("λ".to_owned()),
        ];
        2
    ];
    let (scalar_truths, scalar_rows) = scalar_oracle(&columns, &predicate);
    let functions = HashMap::new();
    let base = TypedBatch::from_columns(&controls, &schema, &columns, 6, None)
        .expect("six-row typed source");

    // Act
    let sliced = base.slice(&controls, 1, 4).expect("nonzero base slice");
    let selected = TypedBatch::from_views(
        &controls,
        sliced.schema(),
        sliced.columns.get(),
        4,
        Some(&[0, 3, 3, 1, 2]),
    )
    .expect("ordered repeated selection over sliced columns");
    let window = selected
        .slice(&controls, 1, 4)
        .expect("nonzero selected slice");

    // Assert
    assert_eq!(sliced.len(), 4);
    assert_eq!(selected.len(), 5);
    assert_eq!(window.len(), 4);
    assert_eq!(window.schema(), &schema);
    assert_eq!(rows(&window), expected_window_rows());
    assert_eq!(
        scalar_truths,
        vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Null,
            Value::Bool(false)
        ]
    );
    assert_eq!(scalar_rows, expected_rows);
    assert_eq!(
        capability::expression(&predicate, window.schema(), &[]),
        Capability::NativeTyped
    );

    // Assert
    let filtered = window
        .filter(&controls, &predicate, &[], &functions, None)
        .expect("native TRUE-only filter");
    let projected = filtered
        .project(&controls, &projection, &[], &functions, None)
        .expect("aliased passthrough gathers");
    drop((base, sliced, selected, window, filtered));
    let retained_bytes = controls.current_query_memory_bytes();
    let alias = projected.clone();

    // Assert
    assert!(retained_bytes > 0);
    assert_eq!(controls.current_query_memory_bytes(), retained_bytes);
    assert_eq!(projected.len(), 2);
    assert_eq!(projected.schema(), &expected_schema);
    assert_eq!(rows(&projected), scalar_rows);
    assert_eq!(rows(&projected), expected_rows);

    // Assert
    drop(projected);

    // Assert
    assert_eq!(controls.current_query_memory_bytes(), retained_bytes);
    assert_eq!(alias.len(), 2);
    assert_eq!(alias.schema(), &expected_schema);
    assert_eq!(rows(&alias), scalar_rows);
    assert_eq!(rows(&alias), expected_rows);
    drop(alias);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

fn source_columns() -> [Vec<Value>; 3] {
    [
        vec![
            Value::Int64(100),
            Value::Int64(10),
            Value::Null,
            Value::Int64(30),
            Value::Int64(40),
            Value::Int64(999),
        ],
        vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Null,
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(true),
        ],
        ["left", "a", "β", "c", "λ", "right"]
            .map(|text| Value::String(text.to_owned()))
            .to_vec(),
    ]
}

fn expected_window_rows() -> Vec<Vec<Value>> {
    vec![
        vec![
            Value::Int64(40),
            Value::Bool(true),
            Value::String("λ".to_owned()),
        ],
        vec![
            Value::Int64(40),
            Value::Bool(true),
            Value::String("λ".to_owned()),
        ],
        vec![Value::Null, Value::Null, Value::String("β".to_owned())],
        vec![
            Value::Int64(30),
            Value::Bool(false),
            Value::String("c".to_owned()),
        ],
    ]
}

fn scalar_oracle(columns: &[Vec<Value>], predicate: &Expr) -> (Vec<Value>, Vec<Vec<Value>>) {
    let functions = HashMap::new();
    let mut truths = Vec::new();
    let mut projected = Vec::new();
    // These original physical positions are independent of every typed view.
    for index in [4, 4, 2, 3] {
        let row = BatchRow::from_projected_values(vec![
            ("n".to_owned(), columns[0][index].clone()),
            ("flag".to_owned(), columns[1][index].clone()),
            ("payload".to_owned(), columns[2][index].clone()),
        ]);
        let truth = evaluate_expr_value(&row, predicate, &[], None, &functions, None, None)
            .expect("original-row scalar predicate");
        if truth == Value::Bool(true) {
            projected.push(vec![
                columns[0][index].clone(),
                columns[0][index].clone(),
                columns[2][index].clone(),
                columns[2][index].clone(),
            ]);
        }
        truths.push(truth);
    }
    (truths, projected)
}

fn rows(batch: &TypedBatch) -> Vec<Vec<Value>> {
    (0..batch.len())
        .map(|lane| {
            (0..batch.schema().len())
                .map(|column| batch.value(column, lane).expect("exact typed lane"))
                .collect()
        })
        .collect()
}
