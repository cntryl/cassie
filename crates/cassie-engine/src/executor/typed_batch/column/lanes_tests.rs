//! Lane-built columns equal value-built columns and admit the same bytes.
use super::*;
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::types::Value;
use std::time::Instant;

const LANES: i64 = 40;

fn controls_with_budget(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

/// Irregular, mostly distinct integers so no compact column form applies.
fn scatter(lane: i64) -> i64 {
    let magnitude = (lane * 7919 + 13) % 1009;
    if lane % 3 == 0 {
        -magnitude
    } else {
        magnitude
    }
}

fn flags(null_every: i64) -> Vec<bool> {
    (0..LANES).map(|lane| lane % null_every != 0).collect()
}

struct Case {
    data_type: DataType,
    lanes: RawLanes,
    values: Vec<Value>,
}

fn cases() -> Vec<Case> {
    let valid = flags(5);
    let mut cases = Vec::new();
    for data_type in [DataType::SmallInt, DataType::Int, DataType::BigInt] {
        let raw = (0..LANES).map(scatter).collect::<Vec<_>>();
        cases.push(Case {
            data_type,
            values: raw
                .iter()
                .zip(&valid)
                .map(|(value, valid)| {
                    if *valid {
                        Value::Int64(*value)
                    } else {
                        Value::Null
                    }
                })
                .collect(),
            lanes: RawLanes::I64 {
                values: raw,
                validity: LaneValidity::Flags(valid.clone()),
            },
        });
    }
    let floats = (0..LANES)
        .map(|lane| f64::from(i32::try_from(scatter(lane)).expect("small")) / 8.0)
        .collect::<Vec<_>>();
    cases.push(Case {
        data_type: DataType::Float,
        values: floats
            .iter()
            .zip(&valid)
            .map(|(value, valid)| {
                if *valid {
                    Value::Float64(*value)
                } else {
                    Value::Null
                }
            })
            .collect(),
        lanes: RawLanes::F64 {
            values: floats,
            validity: LaneValidity::Flags(valid.clone()),
        },
    });
    let bools = (0..LANES)
        .map(|lane| u8::from(scatter(lane) % 2 == 0))
        .collect::<Vec<_>>();
    cases.push(Case {
        data_type: DataType::Boolean,
        values: bools
            .iter()
            .zip(&valid)
            .map(|(value, valid)| {
                if *valid {
                    Value::Bool(*value == 1)
                } else {
                    Value::Null
                }
            })
            .collect(),
        lanes: RawLanes::Bool {
            values: bools,
            validity: LaneValidity::Flags(valid.clone()),
        },
    });
    cases.extend(text_and_document_cases(&valid));
    cases
}

fn text_and_document_cases(valid: &[bool]) -> Vec<Case> {
    let mut cases = Vec::new();
    let words = (0..LANES)
        .map(|lane| format!("v{}-é", scatter(lane)))
        .collect::<Vec<_>>();
    let mut bytes = String::new();
    let mut offsets = vec![0];
    for (word, valid) in words.iter().zip(valid) {
        if *valid {
            bytes.push_str(word);
        }
        offsets.push(bytes.len());
    }
    cases.push(Case {
        data_type: DataType::Text,
        values: words
            .iter()
            .zip(valid)
            .map(|(word, valid)| {
                if *valid {
                    Value::String(word.clone())
                } else {
                    Value::Null
                }
            })
            .collect(),
        lanes: RawLanes::Utf8 {
            bytes,
            offsets,
            validity: LaneValidity::Flags(valid.to_vec()),
        },
    });
    let documents = (0..LANES)
        .map(|lane| {
            if lane % 5 == 0 {
                Value::Null
            } else {
                Value::Json(serde_json::json!({ "n": scatter(lane) }))
            }
        })
        .collect::<Vec<_>>();
    cases.push(Case {
        data_type: DataType::Jsonb,
        values: documents.clone(),
        lanes: RawLanes::Values(documents),
    });
    cases
}

fn empty_lanes(data_type: &DataType) -> RawLanes {
    match data_type {
        DataType::Float => RawLanes::F64 {
            values: Vec::new(),
            validity: LaneValidity::AllValid,
        },
        DataType::Boolean => RawLanes::Bool {
            values: Vec::new(),
            validity: LaneValidity::AllValid,
        },
        DataType::Text => RawLanes::Utf8 {
            bytes: String::new(),
            offsets: vec![0],
            validity: LaneValidity::AllValid,
        },
        DataType::Jsonb => RawLanes::Values(Vec::new()),
        _ => RawLanes::I64 {
            values: Vec::new(),
            validity: LaneValidity::AllValid,
        },
    }
}

fn lane_values(column: &Column) -> Vec<Value> {
    (0..column.len())
        .map(|lane| column.cell(lane).expect("cell").to_owned())
        .collect()
}

#[test]
fn should_build_lane_columns_equal_to_value_columns_for_each_type() {
    // Arrange
    let controls = controls();
    for case in cases() {
        // Act
        let from_values =
            Column::from_values(&controls, &case.data_type, &case.values).expect("value column");
        let from_lanes =
            Column::from_lanes(&controls, &case.data_type, case.lanes).expect("lane column");
        // Assert
        assert_eq!(from_lanes.len(), from_values.len());
        assert_eq!(from_lanes.data_type(), from_values.data_type());
        assert_eq!(lane_values(&from_lanes), case.values);
        assert_eq!(lane_values(&from_lanes), lane_values(&from_values));
    }
}

#[test]
fn should_build_empty_lane_columns_equal_to_empty_value_columns() {
    // Arrange
    let controls = controls();
    for data_type in [
        DataType::BigInt,
        DataType::Float,
        DataType::Boolean,
        DataType::Text,
        DataType::Jsonb,
    ] {
        // Act
        let from_lanes =
            Column::from_lanes(&controls, &data_type, empty_lanes(&data_type)).expect("lanes");
        let from_values = Column::from_values(&controls, &data_type, &[]).expect("values");
        // Assert
        assert_eq!(from_lanes.len(), 0);
        assert_eq!(from_lanes.len(), from_values.len());
        assert!(from_lanes.cell(0).is_err());
    }
}

#[test]
fn should_keep_all_valid_lanes_distinct_from_all_null_lanes() {
    // Arrange
    let controls = controls();
    let all_null = RawLanes::I64 {
        values: vec![0; 3],
        validity: LaneValidity::Flags(vec![false; 3]),
    };
    let all_valid = RawLanes::I64 {
        values: vec![4, 5, 6],
        validity: LaneValidity::AllValid,
    };
    // Act
    let null_column = Column::from_lanes(&controls, &DataType::BigInt, all_null).expect("nulls");
    let valid_column = Column::from_lanes(&controls, &DataType::BigInt, all_valid).expect("valid");
    // Assert
    assert_eq!(lane_values(&null_column), vec![Value::Null; 3]);
    assert_eq!(
        lane_values(&valid_column),
        vec![Value::Int64(4), Value::Int64(5), Value::Int64(6)]
    );
}

#[test]
fn should_release_all_lane_column_memory_after_drop() {
    // Arrange
    let controls = controls();
    for case in cases() {
        // Act
        let column = Column::from_lanes(&controls, &case.data_type, case.lanes).expect("lanes");
        let held = controls.current_query_memory_bytes();
        drop(column);
        // Assert
        assert!(held > 0);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_admit_lane_columns_at_exactly_the_needed_bytes() {
    // Arrange
    for case in cases() {
        let measuring = controls();
        let column = Column::from_lanes(&measuring, &case.data_type, case.lanes.clone())
            .expect("unbounded lanes");
        let needed = measuring.peak_query_memory_bytes();
        drop(column);
        let below = controls_with_budget(needed - 1);
        let exact = controls_with_budget(needed);
        // Act
        let rejected = Column::from_lanes(&below, &case.data_type, case.lanes.clone());
        let admitted = Column::from_lanes(&exact, &case.data_type, case.lanes);
        // Assert
        assert!(matches!(
            rejected,
            Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
        ));
        assert_eq!(below.current_query_memory_bytes(), 0);
        assert!(admitted.is_ok());
        assert_eq!(exact.current_query_memory_bytes(), needed);
    }
}

#[test]
fn should_account_plain_lane_columns_like_value_columns() {
    // Arrange
    let cases = cases();
    let plain = cases
        .iter()
        .find(|case| case.data_type == DataType::BigInt)
        .expect("BigInt case");
    let by_lanes = controls();
    let by_values = controls();
    // Act
    let lane_column = Column::from_lanes(&by_lanes, &plain.data_type, plain.lanes.clone());
    let value_column = Column::from_values(&by_values, &plain.data_type, &plain.values);
    // Assert
    assert!(lane_column.is_ok() && value_column.is_ok());
    assert_eq!(
        by_lanes.current_query_memory_bytes(),
        by_values.current_query_memory_bytes()
    );
}

#[test]
fn should_reject_malformed_lanes_without_retaining_memory() {
    // Arrange
    let controls = controls();
    let malformed = [
        (
            DataType::BigInt,
            RawLanes::I64 {
                values: vec![1, 2],
                validity: LaneValidity::Flags(vec![true]),
            },
        ),
        (
            DataType::SmallInt,
            RawLanes::I64 {
                values: vec![i64::from(i16::MAX) + 1],
                validity: LaneValidity::AllValid,
            },
        ),
        (
            DataType::Boolean,
            RawLanes::Bool {
                values: vec![2],
                validity: LaneValidity::AllValid,
            },
        ),
        (
            DataType::Text,
            RawLanes::Utf8 {
                bytes: "é".to_owned(),
                offsets: vec![0, 1, 2],
                validity: LaneValidity::AllValid,
            },
        ),
        (
            DataType::Text,
            RawLanes::Utf8 {
                bytes: "ab".to_owned(),
                offsets: vec![0, 2, 1],
                validity: LaneValidity::AllValid,
            },
        ),
        (
            DataType::Float,
            RawLanes::I64 {
                values: vec![1],
                validity: LaneValidity::AllValid,
            },
        ),
        (
            DataType::BigInt,
            RawLanes::I64 {
                values: vec![1; 3],
                validity: LaneValidity::Words(vec![0b1000]),
            },
        ),
        (
            DataType::BigInt,
            RawLanes::Values(vec![Value::String("x".to_owned())]),
        ),
    ];
    for (data_type, lanes) in malformed {
        // Act
        let result = Column::from_lanes(&controls, &data_type, lanes);
        // Assert
        assert!(result.is_err());
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_ignore_placeholder_lanes_under_invalid_flags() {
    // Arrange
    let controls = controls();
    let lanes = RawLanes::I64 {
        values: vec![i64::MAX, 7],
        validity: LaneValidity::Flags(vec![false, true]),
    };
    // Act
    let column = Column::from_lanes(&controls, &DataType::SmallInt, lanes).expect("lanes");
    // Assert
    assert_eq!(lane_values(&column), vec![Value::Null, Value::Int64(7)]);
}

#[test]
fn should_normalize_validity_words() {
    // Arrange
    let mixed = [0b101_u64];
    // Act
    let from_words = Validity::from_words(&mixed, 3).expect("mixed");
    let all_valid = Validity::from_words(&[0b111], 3).expect("valid");
    let all_null = Validity::from_words(&[0], 3).expect("null");
    let empty = Validity::from_words(&[], 0).expect("empty");
    // Assert
    assert!(from_words.is_valid(0) && !from_words.is_valid(1) && from_words.is_valid(2));
    assert!(matches!(all_valid, Validity::AllValid));
    assert!(matches!(all_null, Validity::AllNull));
    assert!(matches!(empty, Validity::AllValid));
    assert!(Validity::from_words(&[0b1000], 3).is_none());
    assert!(Validity::from_words(&[1, 2], 3).is_none());
}
