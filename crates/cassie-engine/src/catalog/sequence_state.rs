//! PostgreSQL sequence options and the `nextval`/`setval` state machine.
//!
//! A sequence holds `last_value` and `is_called` exactly as PostgreSQL does:
//! the first `nextval` returns `last_value` itself, later calls step by
//! `increment`, and the bounds come from the sequence's data type unless
//! `MINVALUE`/`MAXVALUE` narrow them.
use serde::{Deserialize, Serialize};

use crate::types::DataType;

/// Option clauses from `CREATE SEQUENCE`, `ALTER SEQUENCE` or an identity
/// column. `None` keeps the current (or default) value.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SequenceOptions {
    pub data_type: Option<DataType>,
    pub increment: Option<i64>,
    /// `Some(None)` is `NO MINVALUE`.
    pub min_value: Option<Option<i64>>,
    /// `Some(None)` is `NO MAXVALUE`.
    pub max_value: Option<Option<i64>>,
    pub start: Option<i64>,
    /// `Some(None)` is `RESTART` without a value.
    pub restart: Option<Option<i64>>,
    pub cache: Option<i64>,
    pub cycle: Option<bool>,
}

/// A sequence failure with PostgreSQL's SQLSTATE and message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceError {
    pub sqlstate: &'static str,
    pub message: String,
}

impl std::fmt::Display for SequenceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SequenceError {}

fn invalid(message: String) -> SequenceError {
    SequenceError {
        sqlstate: "22023",
        message,
    }
}

/// The value range of a sequence data type, and its SQL name.
fn type_range(data_type: &DataType) -> Result<(i64, i64, &'static str), SequenceError> {
    match data_type {
        DataType::SmallInt => Ok((i64::from(i16::MIN), i64::from(i16::MAX), "smallint")),
        DataType::Int => Ok((i64::from(i32::MIN), i64::from(i32::MAX), "integer")),
        DataType::BigInt => Ok((i64::MIN, i64::MAX, "bigint")),
        _ => Err(invalid(
            "sequence type must be smallint, integer, or bigint".to_string(),
        )),
    }
}

/// The resolved parameters of a sequence (`pg_sequence`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SequenceParameters {
    pub data_type: DataType,
    pub start: i64,
    pub increment: i64,
    pub min_value: i64,
    pub max_value: i64,
    pub cache: i64,
    pub cycle: bool,
}

impl SequenceParameters {
    /// Resolves `options` over `current` (or PostgreSQL's defaults), checking
    /// the same rules as PostgreSQL's `init_params`.
    ///
    /// # Errors
    /// Returns `22023` for an invalid combination.
    pub fn resolve(
        current: Option<&Self>,
        options: &SequenceOptions,
        default_type: &DataType,
    ) -> Result<Self, SequenceError> {
        let data_type = options
            .data_type
            .clone()
            .or_else(|| current.map(|current| current.data_type.clone()))
            .unwrap_or_else(|| default_type.clone());
        let (type_min, type_max, type_name) = type_range(&data_type)?;
        let increment = options
            .increment
            .or_else(|| current.map(|current| current.increment))
            .unwrap_or(1);
        if increment == 0 {
            return Err(invalid("INCREMENT must not be zero".to_string()));
        }
        let type_changed = current.is_some_and(|current| current.data_type != data_type);
        let max_value = match options.max_value {
            Some(Some(value)) => value,
            Some(None) => default_max(increment, type_max),
            None => match current {
                Some(current) if !type_changed => current.max_value,
                _ => default_max(increment, type_max),
            },
        };
        let min_value = match options.min_value {
            Some(Some(value)) => value,
            Some(None) => default_min(increment, type_min),
            None => match current {
                Some(current) if !type_changed => current.min_value,
                _ => default_min(increment, type_min),
            },
        };
        if max_value > type_max || max_value < type_min {
            return Err(invalid(format!(
                "MAXVALUE ({max_value}) is out of range for sequence data type {type_name}"
            )));
        }
        if min_value > type_max || min_value < type_min {
            return Err(invalid(format!(
                "MINVALUE ({min_value}) is out of range for sequence data type {type_name}"
            )));
        }
        if min_value >= max_value {
            return Err(invalid(format!(
                "MINVALUE ({min_value}) must be less than MAXVALUE ({max_value})"
            )));
        }
        let start = options
            .start
            .or_else(|| current.map(|current| current.start))
            .unwrap_or(if increment > 0 { min_value } else { max_value });
        check_start("START value", start, min_value, max_value)?;
        let cache = options
            .cache
            .or_else(|| current.map(|current| current.cache))
            .unwrap_or(1);
        if cache <= 0 {
            return Err(invalid(format!(
                "CACHE ({cache}) must be greater than zero"
            )));
        }
        let cycle = options
            .cycle
            .or_else(|| current.map(|current| current.cycle))
            .unwrap_or(false);
        Ok(Self {
            data_type,
            start,
            increment,
            min_value,
            max_value,
            cache,
            cycle,
        })
    }
}

fn default_max(increment: i64, type_max: i64) -> i64 {
    if increment > 0 {
        type_max
    } else {
        -1
    }
}

fn default_min(increment: i64, type_min: i64) -> i64 {
    if increment > 0 {
        1
    } else {
        type_min
    }
}

fn check_start(label: &str, value: i64, min: i64, max: i64) -> Result<(), SequenceError> {
    if value < min {
        return Err(invalid(format!(
            "{label} ({value}) cannot be less than MINVALUE ({min})"
        )));
    }
    if value > max {
        return Err(invalid(format!(
            "{label} ({value}) cannot be greater than MAXVALUE ({max})"
        )));
    }
    Ok(())
}

/// `last_value` and `is_called` of one sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequenceCursor {
    pub last_value: i64,
    pub is_called: bool,
}

impl SequenceCursor {
    #[must_use]
    pub const fn starting_at(start: i64) -> Self {
        Self {
            last_value: start,
            is_called: false,
        }
    }

    /// `nextval`.
    ///
    /// # Errors
    /// Returns `2200H` when a non-cycling sequence is exhausted.
    pub fn next(
        &mut self,
        name: &str,
        parameters: &SequenceParameters,
    ) -> Result<i64, SequenceError> {
        if !self.is_called {
            self.is_called = true;
            return Ok(self.last_value);
        }
        let stepped = self.last_value.checked_add(parameters.increment);
        let next = if parameters.increment > 0 {
            match stepped {
                Some(next) if next <= parameters.max_value => next,
                _ if parameters.cycle => parameters.min_value,
                _ => {
                    return Err(SequenceError {
                        sqlstate: "2200H",
                        message: format!(
                            "nextval: reached maximum value of sequence \"{name}\" ({})",
                            parameters.max_value
                        ),
                    })
                }
            }
        } else {
            match stepped {
                Some(next) if next >= parameters.min_value => next,
                _ if parameters.cycle => parameters.max_value,
                _ => {
                    return Err(SequenceError {
                        sqlstate: "2200H",
                        message: format!(
                            "nextval: reached minimum value of sequence \"{name}\" ({})",
                            parameters.min_value
                        ),
                    })
                }
            }
        };
        self.last_value = next;
        Ok(next)
    }

    /// `setval(sequence, value, is_called)`.
    ///
    /// # Errors
    /// Returns `22003` when `value` lies outside the sequence bounds.
    pub fn set(
        &mut self,
        name: &str,
        parameters: &SequenceParameters,
        value: i64,
        is_called: bool,
    ) -> Result<i64, SequenceError> {
        if value < parameters.min_value || value > parameters.max_value {
            return Err(SequenceError {
                sqlstate: "22003",
                message: format!(
                    "setval: value {value} is out of bounds for sequence \"{name}\" ({}..{})",
                    parameters.min_value, parameters.max_value
                ),
            });
        }
        self.last_value = value;
        self.is_called = is_called;
        Ok(value)
    }

    /// `ALTER SEQUENCE ... RESTART [WITH value]`.
    ///
    /// # Errors
    /// Returns `22023` when the restart value lies outside the bounds.
    pub fn restart(
        &mut self,
        parameters: &SequenceParameters,
        value: Option<i64>,
    ) -> Result<(), SequenceError> {
        let value = value.unwrap_or(parameters.start);
        check_start(
            "RESTART value",
            value,
            parameters.min_value,
            parameters.max_value,
        )?;
        *self = Self::starting_at(value);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{SequenceCursor, SequenceOptions, SequenceParameters};
    use crate::types::DataType;

    fn parameters(options: &SequenceOptions) -> SequenceParameters {
        SequenceParameters::resolve(None, options, &DataType::BigInt).expect("valid options")
    }

    #[test]
    fn should_stop_a_smallint_sequence_at_its_type_maximum() {
        // Arrange
        let parameters = parameters(&SequenceOptions {
            data_type: Some(DataType::SmallInt),
            start: Some(32_766),
            ..SequenceOptions::default()
        });
        let mut cursor = SequenceCursor::starting_at(parameters.start);

        // Act
        let values = [
            cursor.next("s1", &parameters),
            cursor.next("s1", &parameters),
        ];
        let exhausted = cursor
            .next("s1", &parameters)
            .map_err(|error| error.sqlstate);

        // Assert
        assert_eq!(values, [Ok(32_766), Ok(32_767)]);
        assert_eq!(exhausted, Err("2200H"));
    }

    #[test]
    fn should_default_descending_sequence_bounds_like_postgresql() {
        // Arrange
        let options = SequenceOptions {
            increment: Some(-1),
            ..SequenceOptions::default()
        };

        // Act
        let parameters = parameters(&options);

        // Assert
        assert_eq!(
            (parameters.start, parameters.max_value, parameters.min_value),
            (-1, -1, i64::MIN)
        );
    }

    #[test]
    fn should_cycle_back_to_the_minimum() {
        // Arrange
        let parameters = parameters(&SequenceOptions {
            max_value: Some(Some(2)),
            cycle: Some(true),
            ..SequenceOptions::default()
        });
        let mut cursor = SequenceCursor::starting_at(parameters.start);

        // Act
        let values = [(); 3].map(|()| cursor.next("s3", &parameters));

        // Assert
        assert_eq!(values, [Ok(1), Ok(2), Ok(1)]);
    }

    #[test]
    fn should_return_the_set_value_next_when_setval_is_not_called() {
        // Arrange
        let parameters = parameters(&SequenceOptions::default());
        let mut cursor = SequenceCursor::starting_at(parameters.start);

        // Act
        let set = cursor.set("s4", &parameters, 10, false);
        let next = cursor.next("s4", &parameters);

        // Assert
        assert_eq!((set, next), (Ok(10), Ok(10)));
    }

    #[test]
    fn should_reject_invalid_options_with_postgresql_messages() {
        // Arrange
        let cases = [
            SequenceOptions {
                start: Some(5),
                min_value: Some(Some(10)),
                ..SequenceOptions::default()
            },
            SequenceOptions {
                data_type: Some(DataType::Int),
                max_value: Some(Some(3_000_000_000)),
                ..SequenceOptions::default()
            },
        ];

        // Act
        let messages = cases.map(|options| {
            SequenceParameters::resolve(None, &options, &DataType::BigInt)
                .map_err(|error| error.message)
        });

        // Assert
        assert_eq!(
            messages,
            [
                Err("START value (5) cannot be less than MINVALUE (10)".to_string()),
                Err(
                    "MAXVALUE (3000000000) is out of range for sequence data type integer"
                        .to_string()
                ),
            ]
        );
    }
}
