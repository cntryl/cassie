//! Portable persisted sequence declarations and sequence state values.

use serde::{Deserialize, Serialize};

use crate::catalog::declarations::{SequenceOptions, SequencePersistence};
use crate::types::DataType;

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

/// The column that owns a sequence (`OWNED BY table.column`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SequenceOwner {
    pub table: String,
    pub column: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SequenceMeta {
    pub name: String,
    pub data_type: DataType,
    pub start_value: i64,
    pub increment_by: i64,
    /// PostgreSQL's `last_value`.
    pub current_value: i64,
    /// Whether `current_value` has been returned by `nextval`.
    pub is_called: bool,
    pub min_value: i64,
    pub max_value: i64,
    pub cache: i64,
    pub cycle: bool,
    #[serde(default)]
    pub owned_by: Option<SequenceOwner>,
    #[serde(default)]
    pub persistence: SequencePersistence,
    /// Stable relation identity allocated at creation; renames keep it.
    #[serde(default)]
    pub object_id: u64,
}

impl SequenceMeta {
    /// A sequence with PostgreSQL's default options for `data_type`.
    pub fn new(name: impl Into<String>, data_type: DataType) -> Self {
        let parameters = SequenceParameters::resolve(None, &SequenceOptions::default(), &data_type)
            .unwrap_or(SequenceParameters {
                data_type,
                start: 1,
                increment: 1,
                min_value: 1,
                max_value: i64::MAX,
                cache: 1,
                cycle: false,
            });
        Self::with_parameters(name, parameters)
    }

    /// A sequence with resolved `parameters`, positioned at its start.
    pub fn with_parameters(name: impl Into<String>, parameters: SequenceParameters) -> Self {
        Self {
            name: name.into(),
            current_value: parameters.start,
            is_called: false,
            data_type: parameters.data_type,
            start_value: parameters.start,
            increment_by: parameters.increment,
            min_value: parameters.min_value,
            max_value: parameters.max_value,
            cache: parameters.cache,
            cycle: parameters.cycle,
            owned_by: None,
            persistence: SequencePersistence::Permanent,
            object_id: 0,
        }
    }

    #[must_use]
    pub fn parameters(&self) -> SequenceParameters {
        SequenceParameters {
            data_type: self.data_type.clone(),
            start: self.start_value,
            increment: self.increment_by,
            min_value: self.min_value,
            max_value: self.max_value,
            cache: self.cache,
            cycle: self.cycle,
        }
    }

    #[must_use]
    pub const fn cursor(&self) -> SequenceCursor {
        SequenceCursor {
            last_value: self.current_value,
            is_called: self.is_called,
        }
    }

    pub fn set_parameters(&mut self, parameters: SequenceParameters) {
        self.data_type = parameters.data_type;
        self.start_value = parameters.start;
        self.increment_by = parameters.increment;
        self.min_value = parameters.min_value;
        self.max_value = parameters.max_value;
        self.cache = parameters.cache;
        self.cycle = parameters.cycle;
    }

    pub const fn set_cursor(&mut self, cursor: SequenceCursor) {
        self.current_value = cursor.last_value;
        self.is_called = cursor.is_called;
    }

    /// The unqualified relation name.
    #[must_use]
    pub fn local_name(&self) -> String {
        crate::catalog::scope::local_name(&self.name)
    }

    /// `nextval`, applied in place.
    ///
    /// # Errors
    /// Returns `2200H` when a non-cycling sequence is exhausted.
    pub fn advance(&mut self) -> Result<i64, SequenceError> {
        let mut cursor = self.cursor();
        let value = cursor.next(&self.local_name(), &self.parameters())?;
        self.set_cursor(cursor);
        Ok(value)
    }

    /// `setval`, applied in place.
    ///
    /// # Errors
    /// Returns `22003` when `value` is outside the sequence bounds.
    pub fn assign(&mut self, value: i64, is_called: bool) -> Result<i64, SequenceError> {
        let mut cursor = self.cursor();
        let value = cursor.set(&self.local_name(), &self.parameters(), value, is_called)?;
        self.set_cursor(cursor);
        Ok(value)
    }
}
