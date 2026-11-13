//! Validation of `json[b]_to_record[set]`, `json[b]_populate_record[set]` and `JSON_TABLE`.
use super::{validate_expression, CassieError, DataType, Expr, FunctionCall, HashSet};
use crate::app::DiagnosticError;
use crate::sql::ast::{
    JsonBehavior, JsonQuotes, JsonTableColumn, JsonTableValueColumn, JsonWrapper, TableFunctionForm,
};
use crate::sql::table_sources::{self, RecordSource};

fn diagnostic(state: &'static str, message: impl Into<String>) -> CassieError {
    CassieError::from(DiagnosticError::new(state, message))
}

/// Arity, column definition list and base-type rules of the record-returning functions.
pub(super) fn validate_record_function(
    function: &FunctionCall,
    record: RecordSource,
    form: Option<&TableFunctionForm>,
    lateral_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    let name = record.function_name();
    if function.args.len() != record.argument_count() || function.has_modifiers() {
        return Err(diagnostic(
            "42883",
            format!("function {name}(...) does not exist"),
        ));
    }
    for argument in &function.args {
        validate_expression(argument, lateral_fields, &HashSet::new(), false)?;
    }
    if record.populate {
        validate_base(function, &name)?;
        if form.is_some() {
            return Err(diagnostic(
                "42601",
                "a column definition list is redundant for a function returning a named \
                 composite type",
            ));
        }
    } else if !matches!(form, Some(TableFunctionForm::Columns(columns)) if !columns.is_empty()) {
        return Err(diagnostic(
            "42601",
            "a column definition list is required for functions returning \"record\"",
        ));
    }
    Ok(())
}

/// The base argument of `populate_record[set]` supplies the output row type, so it must be a
/// cast to a named composite type.
fn validate_base(function: &FunctionCall, name: &str) -> Result<(), CassieError> {
    match function.args.first() {
        Some(Expr::Cast {
            data_type: DataType::Composite(_),
            ..
        }) => Ok(()),
        Some(Expr::Cast { .. }) => Err(diagnostic(
            "42804",
            format!("first argument of {name} must be a row type"),
        )),
        Some(Expr::Null) => Err(diagnostic(
            "42804",
            "could not determine polymorphic type because input has type unknown",
        )),
        _ => Err(CassieError::Unsupported(format!(
            "{name} requires its first argument to be a cast to a named composite type"
        ))),
    }
}

/// Argument shape, path syntax, name uniqueness and per-column rules of `JSON_TABLE`.
pub(super) fn validate_json_table(
    function: &FunctionCall,
    form: Option<&TableFunctionForm>,
    lateral_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    let Some(TableFunctionForm::JsonTable(spec)) = form else {
        return Err(diagnostic("42601", "JSON_TABLE requires a COLUMNS clause"));
    };
    if function.args.len() != spec.passing.len() + 1 || function.has_modifiers() {
        return Err(diagnostic("42601", "invalid JSON_TABLE call"));
    }
    for argument in &function.args {
        validate_expression(argument, lateral_fields, &HashSet::new(), false)?;
    }
    validate_path(&spec.path)?;
    let mut names = HashSet::new();
    if let Some(name) = &spec.path_name {
        register(&mut names, name)?;
    }
    validate_columns(&spec.columns, &mut names)
}

fn register(names: &mut HashSet<String>, name: &str) -> Result<(), CassieError> {
    if names.insert(name.to_owned()) {
        Ok(())
    } else {
        Err(diagnostic(
            "42712",
            format!("duplicate JSON_TABLE column or path name: {name}"),
        ))
    }
}

fn validate_path(path: &str) -> Result<(), CassieError> {
    sql_json_path::JsonPath::new(path)
        .map(|_| ())
        .map_err(|error| CassieError::Parse(error.to_string()))
}

fn validate_columns(
    columns: &[JsonTableColumn],
    names: &mut HashSet<String>,
) -> Result<(), CassieError> {
    let mut ordinalities = 0_usize;
    for column in columns {
        match column {
            JsonTableColumn::Ordinality { name } => {
                ordinalities += 1;
                if ordinalities > 1 {
                    return Err(diagnostic(
                        "42601",
                        "only one FOR ORDINALITY column is allowed",
                    ));
                }
                register(names, name)?;
            }
            JsonTableColumn::Value(column) => {
                register(names, &column.name)?;
                validate_path(&column.path)?;
                validate_value_column(column)?;
            }
            JsonTableColumn::Exists(column) => {
                register(names, &column.name)?;
                validate_path(&column.path)?;
            }
            JsonTableColumn::Nested(nested) => {
                validate_path(&nested.path)?;
                if let Some(name) = &nested.path_name {
                    register(names, name)?;
                }
                validate_columns(&nested.columns, names)?;
            }
        }
    }
    Ok(())
}

fn string_like(data_type: &DataType) -> bool {
    matches!(
        data_type,
        DataType::Json
            | DataType::Jsonb
            | DataType::Text
            | DataType::Varchar { .. }
            | DataType::Char { .. }
            | DataType::Bytea
    )
}

fn validate_value_column(column: &JsonTableValueColumn) -> Result<(), CassieError> {
    if column.format_json && !string_like(&column.data_type) {
        return Err(CassieError::Unsupported(
            "cannot use JSON format with non-string output types".into(),
        ));
    }
    if matches!(
        column.wrapper,
        JsonWrapper::Conditional | JsonWrapper::Unconditional
    ) && column.quotes != JsonQuotes::Unspecified
    {
        return Err(diagnostic(
            "42601",
            "SQL/JSON QUOTES behavior must not be specified when WITH WRAPPER is used",
        ));
    }
    let query = table_sources::is_query_column(column);
    for behavior in [&column.on_empty, &column.on_error] {
        match behavior {
            JsonBehavior::EmptyArray | JsonBehavior::EmptyObject if !query => {
                return Err(diagnostic(
                    "42601",
                    format!("invalid ON EMPTY behavior for column \"{}\"", column.name),
                ));
            }
            JsonBehavior::Default(expression) if !constant_expression(expression) => {
                return Err(diagnostic(
                    "42804",
                    "can only specify a constant, non-aggregate function, or operator \
                     expression for DEFAULT",
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Literals combined by casts, negation and plain operators.
fn constant_expression(expression: &Expr) -> bool {
    match expression {
        Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::IntegerLiteral(_)
        | Expr::NumericLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null => true,
        Expr::Cast { .. }
        | Expr::Binary { .. }
        | Expr::UnaryNumeric { .. }
        | Expr::IntegerNegate { .. } => expression
            .try_visit_children(|child| {
                if constant_expression(child) {
                    Ok(())
                } else {
                    Err(())
                }
            })
            .is_ok(),
        _ => false,
    }
}
