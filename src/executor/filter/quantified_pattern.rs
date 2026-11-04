//! Ordered quantified LIKE/ILIKE/regex matching over the selected TEXT[] carrier.

use super::{check_interruption, regexp_like_text, reserve_output, resource_limit, QueryError};
use crate::{
    app::CassieError,
    runtime::QueryExecutionControls,
    types::{DataType, SqlArray, Value},
};

pub(super) fn evaluate(
    arguments: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    check_interruption(controls)?;
    let [left, right, Value::String(mode), Value::Bool(negated), Value::Bool(all)] = arguments
    else {
        return Err(
            CassieError::Execution("invalid internal quantified pattern arguments".into()).into(),
        );
    };
    let decoded;
    let _decoded_memory;
    let array = match right {
        Value::Null => return Ok(Value::Null),
        Value::Array(array) => array,
        Value::String(text) => {
            // Unknown TEXT[] input uses the existing native array codec, never
            // the JSON carrier. Keep its conservative decode workspace live.
            let bound = text
                .len()
                .checked_mul(256)
                .and_then(|bytes| bytes.checked_add(4096))
                .ok_or_else(|| resource_limit("pattern array decode size overflow"))?;
            _decoded_memory = reserve_output(controls, bound)?;
            decoded = decode_array(text)?;
            &decoded
        }
        _ => {
            return Err(CassieError::Execution(
                "op ANY/ALL (array) requires array on right side".into(),
            )
            .into())
        }
    };
    let mut unknown = false;
    for pattern in array.elements() {
        check_interruption(controls)?;
        let (Value::String(input), Value::String(pattern)) = (left, pattern) else {
            if matches!(left, Value::Null) || matches!(pattern, Value::Null) {
                unknown = true;
                continue;
            }
            return Err(CassieError::Execution(
                "operator does not exist for pattern operand types".into(),
            )
            .into());
        };
        let matched = match mode.as_str() {
            "regex" | "iregex" => regexp_like_text(
                input,
                pattern,
                if mode == "iregex" { "i" } else { "" },
                controls,
            )?,
            "like" | "ilike" => crate::executor::filter::like::controlled(
                input,
                pattern,
                Some('\\'),
                mode == "ilike",
                || check_interruption(controls),
            )
            .map_err(|error| match error {
                crate::executor::filter::like::MatchError::Pattern => {
                    QueryError::General(crate::executor::filter::like::LikePatternError.to_string())
                }
                crate::executor::filter::like::MatchError::Interrupted(error) => error,
            })?,
            _ => {
                return Err(CassieError::Execution(
                    "invalid internal quantified pattern mode".into(),
                )
                .into())
            }
        };
        let compared = matched != *negated;
        if compared != *all {
            return Ok(Value::Bool(compared));
        }
    }
    check_interruption(controls)?;
    Ok(if unknown {
        Value::Null
    } else {
        Value::Bool(*all)
    })
}

fn decode_array(text: &str) -> Result<SqlArray, QueryError> {
    crate::types::array::parse_sql_text_array(text, &DataType::Text)
        .map_err(|error| CassieError::ArrayInput(error.to_string()))?
        .into_sql_array(&DataType::Text)
        .map_err(|message| CassieError::ArrayInput(message).into())
}

#[cfg(test)]
mod tests {
    use super::evaluate;
    use crate::types::{SqlArray, Value};

    struct Case {
        label: &'static str,
        left_null: bool,
        patterns: Option<&'static [Option<bool>]>,
        expected: [Option<bool>; 4],
    }

    const CASES: [Case; 8] = [
        Case {
            label: "null_array",
            left_null: false,
            patterns: None,
            expected: [None, None, None, None],
        },
        Case {
            label: "empty",
            left_null: false,
            patterns: Some(&[]),
            expected: [Some(false), Some(true), Some(false), Some(true)],
        },
        Case {
            label: "null_only",
            left_null: false,
            patterns: Some(&[None]),
            expected: [None, None, None, None],
        },
        Case {
            label: "match_null",
            left_null: false,
            patterns: Some(&[Some(true), None]),
            expected: [Some(true), None, None, Some(false)],
        },
        Case {
            label: "miss_null",
            left_null: false,
            patterns: Some(&[Some(false), None]),
            expected: [None, Some(false), Some(true), None],
        },
        Case {
            label: "match_miss",
            left_null: false,
            patterns: Some(&[Some(true), Some(false)]),
            expected: [Some(true), Some(false), Some(true), Some(false)],
        },
        Case {
            label: "null_left_empty",
            left_null: true,
            patterns: Some(&[]),
            expected: [Some(false), Some(true), Some(false), Some(true)],
        },
        Case {
            label: "null_left_match",
            left_null: true,
            patterns: Some(&[Some(true)]),
            expected: [None, None, None, None],
        },
    ];

    #[test]
    fn should_preserve_quantified_pattern_three_valued_truth_tables() {
        // Arrange
        let cases = &CASES;
        let mut observations = Vec::new();

        // Act
        for mode in ["like", "ilike", "regex", "iregex"] {
            for case in cases {
                for (slot, (negated, all)) in
                    [(false, false), (false, true), (true, false), (true, true)]
                        .into_iter()
                        .enumerate()
                {
                    let left = if case.left_null {
                        Value::Null
                    } else {
                        Value::String("abc".into())
                    };
                    let right = case.patterns.map_or(Value::Null, |patterns| {
                        let elements = patterns
                            .iter()
                            .map(|matched| {
                                matched.map_or(Value::Null, |matched| {
                                    Value::String(
                                        match (mode, matched) {
                                            ("like" | "ilike", true) => "a%",
                                            ("like" | "ilike", false) => "z%",
                                            (_, true) => "^a",
                                            (_, false) => "^z",
                                        }
                                        .into(),
                                    )
                                })
                            })
                            .collect();
                        Value::Array(
                            SqlArray::from_elements(elements).expect("finite typed fixture"),
                        )
                    });
                    let result = evaluate(
                        &[
                            left,
                            right,
                            Value::String(mode.into()),
                            Value::Bool(negated),
                            Value::Bool(all),
                        ],
                        None,
                    );
                    observations.push((
                        mode,
                        case.label,
                        negated,
                        all,
                        case.expected[slot].map_or(Value::Null, Value::Bool),
                        result,
                    ));
                }
            }
        }

        // Assert
        for (mode, label, negated, all, expected, result) in observations {
            assert_eq!(
                result.expect("pinned quantified truth row"),
                expected,
                "mode={mode}, fixture={label}, negated={negated}, all={all}"
            );
        }
    }
}
