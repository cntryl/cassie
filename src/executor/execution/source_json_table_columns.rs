//! Per-item column evaluation of `JSON_TABLE`: `JSON_VALUE`, `JSON_QUERY` and `JSON_EXISTS`
//! semantics with their `ON EMPTY` and `ON ERROR` behaviors.
use std::collections::HashMap;

use serde_json::Value as Json;

use super::source_json_fragment::{evaluate_argument, Fragment, Kind};
use super::source_json_record_fields::convert;
use super::{CteContext, QueryError, SourceExecutionEnv, Value};
use crate::app::{CassieError, DiagnosticError};
use crate::sql::ast::{
    ExistsBehavior, JsonBehavior, JsonQuotes, JsonTableExistsColumn, JsonTableValueColumn,
    JsonWrapper,
};
use crate::sql::table_sources;
use crate::types::DataType;

pub(super) fn diagnostic(state: &'static str, message: impl Into<String>) -> QueryError {
    QueryError::from(CassieError::from(DiagnosticError::new(state, message)))
}

fn owned(item: sql_json_path::json::Cow<'_, Json>) -> Json {
    item.into_owned()
}

/// The outcome of evaluating one column against one item before behaviors apply.
enum Step {
    Value(Value),
    Empty,
    Soft(QueryError),
}

/// Evaluates paths against items with the `PASSING` variables and a compiled-path cache.
pub(super) struct Evaluator<'a, 'e> {
    env: &'a SourceExecutionEnv<'e>,
    ctes: &'a CteContext,
    vars: Json,
    paths: HashMap<String, sql_json_path::JsonPath>,
}

impl<'a, 'e> Evaluator<'a, 'e> {
    pub(super) fn new(env: &'a SourceExecutionEnv<'e>, ctes: &'a CteContext, vars: Json) -> Self {
        Self {
            env,
            ctes,
            vars,
            paths: HashMap::new(),
        }
    }

    /// The items selected by `path`; a recoverable evaluation error is returned as `Err`.
    pub(super) fn items(
        &mut self,
        path: &str,
        item: &Json,
    ) -> Result<Result<Vec<Json>, String>, QueryError> {
        self.env.controls.check_interruption()?;
        if !self.paths.contains_key(path) {
            let compiled = sql_json_path::JsonPath::new(path)
                .map_err(|error| CassieError::Parse(error.to_string()))?;
            self.paths.insert(path.to_owned(), compiled);
        }
        let Some(compiled) = self.paths.get(path) else {
            return Err(QueryError::General("JSON path cache miss".into()));
        };
        match compiled.query_with_vars(item, &self.vars) {
            Ok(items) => Ok(Ok(items.into_iter().map(owned).collect())),
            Err(error) if error.can_silent() => Ok(Err(error.to_string())),
            Err(error) => Err(CassieError::Execution(error.to_string()).into()),
        }
    }

    /// Value of a `JSON_VALUE` or `JSON_QUERY` column.
    pub(super) fn value_column(
        &mut self,
        column: &JsonTableValueColumn,
        item: &Json,
    ) -> Result<Value, QueryError> {
        let step = self.value_step(column, item)?;
        match step {
            Step::Value(value) => Ok(value),
            Step::Empty => self.behavior(column, &column.on_empty, None),
            Step::Soft(error) => self.behavior(column, &column.on_error, Some(error)),
        }
    }

    fn value_step(
        &mut self,
        column: &JsonTableValueColumn,
        item: &Json,
    ) -> Result<Step, QueryError> {
        let items = match self.items(&column.path, item)? {
            Ok(items) => items,
            Err(message) => return Ok(Step::Soft(diagnostic("22039", message))),
        };
        if table_sources::is_query_column(column) {
            query_step(column, items)
        } else {
            value_step(column, items)
        }
    }

    /// Applies an `ON EMPTY` or `ON ERROR` behavior.
    fn behavior(
        &mut self,
        column: &JsonTableValueColumn,
        behavior: &JsonBehavior,
        error: Option<QueryError>,
    ) -> Result<Value, QueryError> {
        match behavior {
            JsonBehavior::Unspecified | JsonBehavior::Null => Ok(Value::Null),
            JsonBehavior::Error => Err(error.unwrap_or_else(|| {
                diagnostic(
                    "22035",
                    format!(
                        "no SQL/JSON item found for specified path of column \"{}\"",
                        column.name
                    ),
                )
            })),
            JsonBehavior::EmptyArray => {
                convert_item(column, Json::Array(Vec::new()), JsonQuotes::Keep)
            }
            JsonBehavior::EmptyObject => convert_item(
                column,
                Json::Object(serde_json::Map::new()),
                JsonQuotes::Keep,
            ),
            JsonBehavior::Default(expression) => {
                let value = evaluate_argument(self.env, expression, None, self.ctes)?;
                super::filter::cast_value(&value, &column.data_type)
            }
        }
    }

    /// Evaluates every `DEFAULT` expression once so a bad default fails even when unused.
    pub(super) fn validate_defaults(
        &self,
        column: &JsonTableValueColumn,
    ) -> Result<(), QueryError> {
        for behavior in [&column.on_empty, &column.on_error] {
            if let JsonBehavior::Default(expression) = behavior {
                let value = evaluate_argument(self.env, expression, None, self.ctes)?;
                super::filter::cast_value(&value, &column.data_type)?;
            }
        }
        Ok(())
    }

    /// Value of an `EXISTS` column.
    pub(super) fn exists_column(
        &mut self,
        column: &JsonTableExistsColumn,
        item: &Json,
    ) -> Result<Value, QueryError> {
        let found = match self.items(&column.path, item)? {
            Ok(items) => Value::Bool(!items.is_empty()),
            Err(message) => match column.on_error {
                ExistsBehavior::Error => return Err(diagnostic("22039", message)),
                ExistsBehavior::True => Value::Bool(true),
                ExistsBehavior::Unknown => Value::Null,
                ExistsBehavior::False | ExistsBehavior::Unspecified => Value::Bool(false),
            },
        };
        if matches!(column.data_type, DataType::Bool) || found.is_null() {
            return Ok(found);
        }
        super::filter::cast_value(&found, &column.data_type)
    }
}

/// `JSON_VALUE`: exactly one scalar item.
fn value_step(column: &JsonTableValueColumn, mut items: Vec<Json>) -> Result<Step, QueryError> {
    if items.is_empty() {
        return Ok(Step::Empty);
    }
    let single = items.len() == 1 && !matches!(items[0], Json::Array(_) | Json::Object(_));
    if !single {
        return Ok(Step::Soft(diagnostic(
            "22034",
            format!(
                "JSON path expression for column \"{}\" must return single scalar item",
                column.name
            ),
        )));
    }
    let item = items.remove(0);
    if item.is_null() {
        return Ok(Step::Value(Value::Null));
    }
    Ok(match convert_item(column, item, JsonQuotes::Omit) {
        Ok(value) => Step::Value(value),
        Err(error) => Step::Soft(error),
    })
}

/// `JSON_QUERY`: wrapper handling, then conversion with the quotes behavior.
fn query_step(column: &JsonTableValueColumn, mut items: Vec<Json>) -> Result<Step, QueryError> {
    if items.is_empty() {
        return Ok(Step::Empty);
    }
    let wrap = match column.wrapper {
        JsonWrapper::Unconditional => true,
        JsonWrapper::Conditional => {
            items.len() > 1 || !matches!(items[0], Json::Array(_) | Json::Object(_))
        }
        JsonWrapper::Unspecified | JsonWrapper::Without => false,
    };
    let item = if wrap {
        Json::Array(items)
    } else if items.len() == 1 {
        items.remove(0)
    } else {
        return Ok(Step::Soft(diagnostic(
            "22034",
            format!(
                "JSON path expression for column \"{}\" must return single item when no \
                 wrapper is requested",
                column.name
            ),
        )));
    };
    let quotes = if column.quotes == JsonQuotes::Omit {
        JsonQuotes::Omit
    } else {
        JsonQuotes::Keep
    };
    Ok(match convert_item(column, item, quotes) {
        Ok(value) => Step::Value(value),
        Err(error) => Step::Soft(error),
    })
}

/// Converts one selected item into the column type.
fn convert_item(
    column: &JsonTableValueColumn,
    item: Json,
    quotes: JsonQuotes,
) -> Result<Value, QueryError> {
    let query = table_sources::is_query_column(column);
    let textual = matches!(
        column.data_type,
        DataType::Text | DataType::Varchar { .. } | DataType::Char { .. }
    );
    if query && textual {
        let fragment = Fragment::Tree(item);
        let text = match (fragment.kind(), quotes) {
            (Kind::String, JsonQuotes::Omit) => fragment.scalar_text()?,
            _ => fragment.json_text()?,
        };
        return super::filter::cast_value(&Value::String(text), &column.data_type);
    }
    if item.is_null() && matches!(column.data_type, DataType::Json | DataType::Jsonb) {
        return Ok(if column.data_type == DataType::Json {
            Value::String("null".into())
        } else {
            Value::Json(Json::Null)
        });
    }
    convert(Fragment::Tree(item), &column.data_type, None, &column.name)
}
