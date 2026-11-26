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

#[test]
fn should_retain_the_original_logical_selection_for_a_full_slice() {
    // Arrange
    let controls = super::controls();
    let schema = [("n".to_owned(), DataType::BigInt)];
    let values = [vec![Value::Int64(10), Value::Null, Value::Int64(30)]];
    let positions = [2, 0, 2, 1];
    let batch = TypedBatch::from_columns(&controls, &schema, &values, 3, Some(&positions))
        .expect("nonmonotone selection with a repeated position");
    let admitted = controls.current_query_memory_bytes();

    // Act
    let full = batch.slice(&controls, 0, batch.len()).expect("full slice");
    let shared_bytes = controls.current_query_memory_bytes();
    drop(batch);
    let retained_bytes = controls.current_query_memory_bytes();
    let selected_positions: [usize; 4] =
        std::array::from_fn(|lane| full.position(lane).expect("logical lane"));
    let selected_rows = rows(&full);
    let selected_schema = full.schema().to_vec();
    drop(full);

    // Assert
    assert!(admitted > 0);
    assert_eq!(shared_bytes, admitted);
    assert_eq!(retained_bytes, admitted);
    assert_eq!(selected_schema, schema);
    assert_eq!(selected_positions, positions);
    assert_eq!(
        selected_rows,
        [
            vec![Value::Int64(30)],
            vec![Value::Int64(10)],
            vec![Value::Int64(30)],
            vec![Value::Null],
        ],
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_slice_a_full_plain_batch_without_additional_admission() {
    // Arrange
    let controls = super::controls();
    let schema = [("n".to_owned(), DataType::BigInt)];
    let values = [vec![Value::Int64(10), Value::Null]];
    let batch =
        TypedBatch::from_columns(&controls, &schema, &values, 2, None).expect("plain batch");
    let admitted = controls.current_query_memory_bytes();
    let headroom = controls
        .reserve_query_memory(controls.query_memory_budget_bytes - admitted)
        .expect("reserve the remaining query budget");

    // Act
    let full = batch.slice(&controls, 0, batch.len());
    drop(headroom);

    // Assert
    let full = full.expect("no additional budget needed for a full slice");
    assert_eq!(rows(&full), [vec![Value::Int64(10)], vec![Value::Null]]);
    assert_eq!(controls.current_query_memory_bytes(), admitted);
}

#[test]
fn should_check_full_slice_cancellation_after_validating_bounds() {
    // Arrange
    use crate::app::CassieError;
    use crate::config::CassieRuntimeLimits;
    use crate::executor::QueryError;
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        std::time::Instant::now(),
        cancellation.clone(),
    );
    let schema = [("n".to_owned(), DataType::BigInt)];
    let batch = TypedBatch::from_columns(&controls, &schema, &[vec![Value::Null]], 1, None)
        .expect("plain batch");

    // Act
    cancellation.cancel();
    let valid_range = batch.slice(&controls, 0, batch.len());
    let invalid_range = batch.slice(&controls, 1, batch.len());

    // Assert
    assert!(matches!(
        valid_range,
        Err(QueryError::Cassie(CassieError::QueryCanceled))
    ));
    assert!(matches!(
        invalid_range,
        Err(QueryError::General(message))
            if message == "invalid typed batch: batch slice is outside logical domain"
    ));
}
