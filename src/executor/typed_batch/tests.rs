#[path = "tests/relational.rs"]
mod relational;

#[path = "tests/boundaries.rs"]
mod boundaries;

#[path = "tests/families.rs"]
pub(super) mod families;

use super::TypedBatch;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use crate::types::{DataType, Value};
use std::time::Instant;

#[test]
fn should_match_scalar_dispatch_for_supported_predicates() {
    // Arrange
    use crate::sql::ast::{BinaryOp, Expr};
    let controls = controls();
    let functions = std::collections::HashMap::new();
    let env = super::scalar::Environment {
        controls: &controls,
        params: &[Value::Float64(9_007_199_254_740_992.0)],
        functions: &functions,
        session: None,
    };
    let batch = TypedBatch::from_columns(
        &controls,
        &[
            ("flag".to_owned(), DataType::Boolean),
            ("n".to_owned(), DataType::BigInt),
        ],
        &[
            vec![Value::Bool(true), Value::Bool(false), Value::Null],
            vec![
                Value::Int64(9_007_199_254_740_993),
                Value::Int64(i64::MIN),
                Value::Null,
            ],
        ],
        3,
        Some(&[2, 0, 1, 0]),
    )
    .expect("selected source");

    // Act
    for op in [BinaryOp::And, BinaryOp::Or] {
        for right in [
            Expr::BoolLiteral(true),
            Expr::BoolLiteral(false),
            Expr::Null,
        ] {
            let expr = Expr::Binary {
                left: Box::new(Expr::Column("flag".to_owned())),
                op: op.clone(),
                right: Box::new(right),
            };
            for lane in 0..batch.len() {
                let scalar = env.evaluate(&batch, lane, &expr).expect("scalar Boolean");
                let native = super::operations::native_value(&batch, lane, &expr, env.params)
                    .expect("native Boolean")
                    .to_owned();
                // Assert
                assert_eq!(&native, scalar.get());
            }
        }
    }
    for op in [
        BinaryOp::Eq,
        BinaryOp::NotEq,
        BinaryOp::Lt,
        BinaryOp::Lte,
        BinaryOp::Gt,
        BinaryOp::Gte,
    ] {
        let expr = Expr::Binary {
            left: Box::new(Expr::Column("n".to_owned())),
            op,
            right: Box::new(Expr::Param(0)),
        };
        assert_eq!(
            super::capability::expression(&expr, batch.schema(), env.params),
            super::capability::Capability::NativeTyped
        );
        for lane in 0..batch.len() {
            let scalar = env.evaluate(&batch, lane, &expr).expect("scalar numeric");
            let native = super::operations::native_value(&batch, lane, &expr, env.params)
                .expect("native numeric")
                .to_owned();
            assert_eq!(&native, scalar.get());
        }
    }
}

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_preserve_typed_transport_identity_through_repeated_projection() {
    // Arrange
    let controls = controls();
    let families = families::logical_families();
    let schema = families
        .iter()
        .enumerate()
        .map(|(index, (data_type, _))| (format!("c{index}"), data_type.clone()))
        .collect::<Vec<_>>();
    let values = families
        .iter()
        .map(|(_, values)| values.clone())
        .collect::<Vec<_>>();
    let batch = TypedBatch::from_columns(&controls, &schema, &values, 3, Some(&[2, 0, 2, 1]))
        .expect("all families");
    let projection = schema
        .iter()
        .map(|(name, _)| crate::sql::ast::SelectItem::Column {
            name: name.clone(),
            alias: None,
        })
        .collect::<Vec<_>>();

    // Act
    let output = batch
        .project(
            &controls,
            &projection,
            &[],
            &std::collections::HashMap::new(),
            None,
        )
        .expect("lossless projection");
    drop(batch);

    // Assert
    assert_eq!(output.schema(), schema);
    for (column, (_, values)) in families.iter().enumerate() {
        for (lane, original) in [2, 0, 2, 1].into_iter().enumerate() {
            let actual = output.value(column, lane).expect("lane");
            if let (Value::Float64(actual), Value::Float64(expected)) = (&actual, &values[original])
            {
                assert_eq!(actual.to_bits(), expected.to_bits());
            } else {
                assert_eq!(actual, values[original]);
            }
        }
    }
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_bound_parameter_identity_in_computed_projection() {
    // Arrange
    let controls = controls();
    let batch = TypedBatch::from_columns(&controls, &[], &[], 1, None).expect("empty tuple");
    let projection = [crate::sql::ast::SelectItem::Expr {
        expr: crate::sql::ast::Expr::Param(0),
        alias: Some("bound".to_owned()),
    }];
    let params = [Value::String("λ".to_owned())];

    // Act
    let output = batch
        .project(
            &controls,
            &projection,
            &params,
            &std::collections::HashMap::new(),
            None,
        )
        .expect("bound projection");

    // Assert
    assert_eq!(output.schema(), &[("bound".to_owned(), DataType::Text)]);
    assert_eq!(output.value(0, 0).expect("bound value"), params[0]);
}

#[test]
fn should_admit_parameter_carrier_clones_before_scalar_evaluation() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 8192,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let batch = TypedBatch::from_columns(&controls, &[], &[], 1, None).expect("empty tuple");
    let params = [Value::String("x".repeat(16_384))];
    let functions = std::collections::HashMap::new();
    let env = super::scalar::Environment {
        controls: &controls,
        params: &params,
        functions: &functions,
        session: None,
    };
    let source_bytes = controls.current_query_memory_bytes();

    // Act
    let result = env.evaluate(&batch, 0, &crate::sql::ast::Expr::Param(0));

    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), source_bytes);
}

#[test]
fn should_admit_projection_descriptor_copy_overlap_before_cloning_an_alias() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 8192,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let batch = TypedBatch::from_columns(
        &controls,
        &[("n".to_owned(), DataType::BigInt)],
        &[vec![Value::Int64(1)]],
        1,
        None,
    )
    .expect("source");
    let projection = [crate::sql::ast::SelectItem::Column {
        name: "n".to_owned(),
        alias: Some("a".repeat(7000)),
    }];
    let source_bytes = controls.current_query_memory_bytes();

    // Act
    let result = batch.project(
        &controls,
        &projection,
        &[],
        &std::collections::HashMap::new(),
        None,
    );

    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), source_bytes);
    assert_eq!(
        batch.value(0, 0).expect("source remains usable"),
        Value::Int64(1)
    );
}

#[test]
fn should_admit_large_scalar_literals_before_evaluating_a_selected_lane() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 8192,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let batch = TypedBatch::from_columns(&controls, &[], &[], 1, None).expect("empty tuple");
    let functions = std::collections::HashMap::new();
    let env = super::scalar::Environment {
        controls: &controls,
        params: &[],
        functions: &functions,
        session: None,
    };
    let expr = crate::sql::ast::Expr::StringLiteral("x".repeat(16_384));
    let source_bytes = controls.current_query_memory_bytes();

    // Act
    let output = env.evaluate(&batch, 0, &expr);

    // Assert
    assert!(output.is_err());
    assert_eq!(controls.current_query_memory_bytes(), source_bytes);
}

#[test]
fn should_preserve_selected_nullable_bigint_positions() {
    // Arrange
    let controls = controls();
    let schema = vec![("n".to_owned(), DataType::BigInt)];
    let columns = vec![vec![
        Value::Int64(9_007_199_254_740_993),
        Value::Null,
        Value::Int64(i64::MAX),
    ]];

    // Act
    let batch = TypedBatch::from_columns(&controls, &schema, &columns, 3, Some(&[2, 0, 2, 1]))
        .expect("checked typed construction");
    let values = (0..4)
        .map(|lane| batch.value(0, lane).expect("selected lane"))
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(
        values,
        vec![
            Value::Int64(i64::MAX),
            Value::Int64(9_007_199_254_740_993),
            Value::Int64(i64::MAX),
            Value::Null
        ]
    );
    assert!(controls.current_query_memory_bytes() > 0);
    drop(batch);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_selected_text_backing_admitted_until_the_last_alias_drops() {
    // Arrange
    let controls = controls();
    let column = super::Column::from_values(
        &controls,
        &DataType::Varchar { length: None },
        &[
            Value::String("λ".to_owned()),
            Value::Null,
            Value::String(String::new()),
        ],
    )
    .expect("native text column");
    let source_bytes = controls.current_query_memory_bytes();

    // Act
    let slice = column.slice(&controls, 1, 2).expect("checked slice");
    let selected = slice.gather(&controls, &[1, 0, 1]).expect("checked gather");
    let alias = selected.clone();
    drop(column);
    drop(slice);
    drop(selected);

    // Assert
    assert!(controls.current_query_memory_bytes() >= source_bytes);
    assert_eq!(alias.data_type(), &DataType::Varchar { length: None });
    assert_eq!(
        alias.cell(0).expect("empty text").to_owned(),
        Value::String(String::new())
    );
    assert_eq!(alias.cell(1).expect("SQL NULL").to_owned(), Value::Null);
    drop(alias);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_source_lease_when_a_selection_map_exceeds_the_remaining_budget() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 1024,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let column = super::Column::from_values(&controls, &DataType::BigInt, &[Value::Int64(7)])
        .expect("source fits");
    let source_bytes = controls.current_query_memory_bytes();
    let blocker = controls
        .reserve_query_memory(1024 - source_bytes)
        .expect("fill remaining budget");

    // Act
    let selected = column.gather(&controls, &[0]);

    // Assert
    assert!(selected.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 1024);
    drop(blocker);
    assert_eq!(controls.current_query_memory_bytes(), source_bytes);
    assert_eq!(
        column.cell(0).expect("source remains usable").to_owned(),
        Value::Int64(7)
    );
    drop(column);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_a_short_base_column_even_when_emitted_lengths_match() {
    // Arrange
    let controls = controls();
    let schema = vec![("n".to_owned(), DataType::BigInt)];
    let columns = vec![vec![Value::Int64(10), Value::Int64(20)]];

    // Act
    let batch = TypedBatch::from_columns(&controls, &schema, &columns, 3, Some(&[2, 0]));

    // Assert
    assert!(batch.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_distinguish_one_zero_column_row_from_an_empty_batch() {
    // Arrange
    let controls = controls();

    // Act
    let one = TypedBatch::from_columns(&controls, &[], &[], 1, None).expect("one empty tuple");
    let zero = TypedBatch::from_columns(&controls, &[], &[], 0, None).expect("zero empty tuples");

    // Assert
    assert_eq!(one.len(), 1);
    assert_eq!(zero.len(), 0);
    assert_eq!(one.schema(), []);
    drop((one, zero));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_compact_carrier_logical_domains() {
    // Arrange
    let controls = controls();
    let entries = super::Column::from_values(
        &controls,
        &DataType::BigInt,
        &[Value::Int64(30), Value::Null, Value::Int64(10)],
    )
    .expect("dictionary entries");

    // Act
    let constant = super::Column::constant(&controls, &DataType::BigInt, &Value::Int64(10), 4)
        .expect("constant");
    let sequence =
        super::Column::sequence(&controls, &DataType::BigInt, 10, 10, 3).expect("sequence");
    let dictionary = entries
        .dictionary(&controls, &[0, 2, 1, 0], &[true, true, true, false])
        .expect("dictionary");

    // Assert
    assert_eq!(constant.len(), 4);
    assert_eq!(
        constant.cell(3).expect("constant tail").to_owned(),
        Value::Int64(10)
    );
    assert_eq!(
        sequence.cell(2).expect("sequence tail").to_owned(),
        Value::Int64(30)
    );
    assert_eq!(
        (0..4)
            .map(|lane| dictionary.cell(lane).expect("dictionary lane").to_owned())
            .collect::<Vec<_>>(),
        vec![Value::Int64(30), Value::Int64(10), Value::Null, Value::Null]
    );
    drop((entries, constant, sequence, dictionary));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_invalid_dictionary_codes_for_null_rows() {
    // Arrange
    let controls = controls();
    let entries = super::Column::from_values(
        &controls,
        &DataType::BigInt,
        &[Value::Int64(1), Value::Int64(2)],
    )
    .expect("entries");
    let retained = controls.current_query_memory_bytes();

    // Act
    let malformed = entries.dictionary(&controls, &[0, 2, 1], &[true, false, true]);

    // Assert
    assert!(malformed.is_err());
    assert_eq!(controls.current_query_memory_bytes(), retained);
}

#[test]
fn should_reject_sequence_overflow_before_selection() {
    // Arrange
    let controls = controls();

    // Act
    let overflow = super::Column::sequence(&controls, &DataType::BigInt, i64::MAX - 1, 1, 3);

    // Assert
    assert!(overflow.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_physical_identity_through_selected_typed_transport() {
    // Arrange
    let controls = controls();
    let nan = f64::from_bits(0x7ff8_0000_0000_0007);
    let floats = vec![
        Value::Float64(-0.0),
        Value::Float64(nan),
        Value::Float64(f64::from_bits(1)),
    ];
    let array = Value::Json(serde_json::json!([9_007_199_254_740_993_i64, null]));
    let array_type = DataType::Array(Box::new(DataType::BigInt));

    // Act
    let batch = TypedBatch::from_columns(
        &controls,
        &[
            ("f".to_owned(), DataType::Float),
            ("a".to_owned(), array_type.clone()),
        ],
        &[floats, vec![array.clone(), Value::Null, array.clone()]],
        3,
        Some(&[2, 0, 1]),
    )
    .expect("lossless columns");

    // Assert
    let bits = (0..3)
        .map(|lane| match batch.value(0, lane).expect("float lane") {
            Value::Float64(value) => value.to_bits(),
            other => panic!("unexpected float carrier {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(bits, vec![1, (-0.0_f64).to_bits(), nan.to_bits()]);
    assert_eq!(batch.value(1, 0).expect("exact array"), array);
    assert_eq!(batch.schema()[1].1, array_type);
}

#[test]
fn should_apply_selection_once_through_filtered_projection() {
    // Arrange
    use crate::sql::ast::{BinaryOp, Expr, SelectItem};
    let controls = controls();
    let batch = TypedBatch::from_columns(
        &controls,
        &[
            ("n".to_owned(), DataType::BigInt),
            ("flag".to_owned(), DataType::Boolean),
        ],
        &[
            vec![Value::Int64(10), Value::Null, Value::Int64(30)],
            vec![Value::Bool(true), Value::Bool(false), Value::Null],
        ],
        3,
        Some(&[2, 0, 2, 1]),
    )
    .expect("selected source");
    let predicate = Expr::Binary {
        left: Box::new(Expr::Binary {
            left: Box::new(Expr::Column("n".to_owned())),
            op: BinaryOp::Gte,
            right: Box::new(Expr::IntegerLiteral(20)),
        }),
        op: BinaryOp::Or,
        right: Box::new(Expr::Column("flag".to_owned())),
    };
    let functions = std::collections::HashMap::new();
    let projection = vec![
        SelectItem::Column {
            name: "n".to_owned(),
            alias: Some("x".to_owned()),
        },
        SelectItem::Column {
            name: "n".to_owned(),
            alias: Some("y".to_owned()),
        },
    ];

    // Act
    let filtered = batch
        .filter(&controls, &predicate, &[], &functions, None)
        .expect("TRUE-only filter");
    let projected = filtered
        .project(&controls, &projection, &[], &functions, None)
        .expect("shared projection");
    drop((batch, filtered));

    // Assert
    assert_eq!(projected.len(), 3);
    assert_eq!(
        projected.schema(),
        &[
            ("x".to_owned(), DataType::BigInt),
            ("y".to_owned(), DataType::BigInt)
        ]
    );
    for column in 0..2 {
        assert_eq!(
            (0..3)
                .map(|lane| projected.value(column, lane).expect("projected lane"))
                .collect::<Vec<_>>(),
            vec![Value::Int64(30), Value::Int64(10), Value::Int64(30)]
        );
    }
    drop(projected);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_use_bounded_case_fallback_only_for_demanded_selected_lanes() {
    // Arrange
    use crate::sql::ast::{BinaryOp, Expr, SelectItem};
    let controls = controls();
    let batch = TypedBatch::from_columns(
        &controls,
        &[
            ("n".to_owned(), DataType::BigInt),
            ("keep".to_owned(), DataType::Boolean),
        ],
        &[
            vec![Value::Int64(0), Value::Int64(2), Value::Int64(0)],
            vec![Value::Bool(true), Value::Bool(true), Value::Bool(false)],
        ],
        3,
        None,
    )
    .expect("source");
    let case = Expr::Case {
        operand: None,
        branches: vec![(
            Expr::Binary {
                left: Box::new(Expr::Column("n".to_owned())),
                op: BinaryOp::Eq,
                right: Box::new(Expr::IntegerLiteral(0)),
            },
            Expr::IntegerLiteral(7),
        )],
        else_expr: Some(Box::new(Expr::Binary {
            left: Box::new(Expr::IntegerLiteral(10)),
            op: BinaryOp::Div,
            right: Box::new(Expr::Column("n".to_owned())),
        })),
    };
    let functions = std::collections::HashMap::new();

    // Act
    let filtered = batch
        .filter(
            &controls,
            &Expr::Column("keep".to_owned()),
            &[],
            &functions,
            None,
        )
        .expect("native Boolean filter");
    let projected = filtered
        .project(
            &controls,
            &[SelectItem::Expr {
                expr: case,
                alias: Some("result".to_owned()),
            }],
            &[],
            &functions,
            None,
        )
        .expect("guarded CASE");

    // Assert
    assert_eq!(projected.len(), 2);
    assert_eq!(
        projected.value(0, 0).expect("guarded zero").as_i64(),
        Some(7)
    );
    assert_eq!(projected.value(0, 1).expect("division").as_i64(), Some(5));
}

#[test]
fn should_match_scalar_bag_across_selected_batch_boundaries() {
    // Arrange
    use crate::sql::ast::{BinaryOp, Expr, SelectItem};
    let controls = controls();
    let schema = vec![
        ("n".to_owned(), DataType::BigInt),
        ("flag".to_owned(), DataType::Boolean),
    ];
    let columns = [
        (0..1025)
            .map(|index| {
                if index % 11 == 0 {
                    Value::Null
                } else {
                    Value::Int64(index)
                }
            })
            .collect::<Vec<_>>(),
        (0..1025)
            .map(|index| match index % 3 {
                0 => Value::Null,
                1 => Value::Bool(true),
                _ => Value::Bool(false),
            })
            .collect::<Vec<_>>(),
    ];
    let predicate = Expr::Binary {
        left: Box::new(Expr::Column("flag".to_owned())),
        op: BinaryOp::Or,
        right: Box::new(Expr::Binary {
            left: Box::new(Expr::Column("n".to_owned())),
            op: BinaryOp::Gt,
            right: Box::new(Expr::IntegerLiteral(1000)),
        }),
    };
    let projection = [
        SelectItem::Column {
            name: "n".to_owned(),
            alias: Some("x".to_owned()),
        },
        SelectItem::Column {
            name: "n".to_owned(),
            alias: Some("y".to_owned()),
        },
    ];
    let functions = std::collections::HashMap::new();
    let expected = boundaries::scalar_bag(&columns, &predicate);

    // Act
    for size in [0, 1, 7, 64, 1023, 1024, 1025] {
        let mut actual = Vec::new();
        let width = size.max(1);
        let mut start = 0;
        loop {
            let end = if size == 0 {
                start
            } else {
                (start + width).min(1025)
            };
            let input = columns
                .iter()
                .map(|column| column[start..end].to_vec())
                .collect::<Vec<_>>();
            let batch = TypedBatch::from_columns(&controls, &schema, &input, end - start, None)
                .expect("typed input");
            let filtered = batch
                .filter(&controls, &predicate, &[], &functions, None)
                .expect("typed filter");
            let projected = filtered
                .project(&controls, &projection, &[], &functions, None)
                .expect("typed projection");
            for lane in 0..projected.len() {
                actual.push(vec![
                    projected.value(0, lane).expect("x"),
                    projected.value(1, lane).expect("y"),
                ]);
            }
            if size == 0 || end == 1025 {
                break;
            }
            start = end;
        }

        // Assert
        assert_eq!(
            actual,
            if size == 0 {
                Vec::new()
            } else {
                expected.clone()
            },
            "batch size {size}"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
