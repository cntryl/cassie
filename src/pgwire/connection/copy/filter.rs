use crate::app::{Cassie, CassieError, CassieSession, CopyColumn};
use crate::sql::ast::CopyStatement;
use crate::types::{DataType, Value};

pub(super) fn filter_rows(
    cassie: &Cassie,
    session: &CassieSession,
    statement: &CopyStatement,
    rows: Vec<Vec<Option<String>>>,
    predicate: &str,
) -> Result<Vec<Vec<Option<String>>>, CassieError> {
    let schema = cassie
        .catalog
        .get_schema(&statement.table)
        .ok_or_else(|| CassieError::CollectionNotFound(statement.table.clone()))?;
    let columns = crate::app::copy_columns(statement, &schema.fields)?;
    let names = columns
        .iter()
        .map(|column| match column {
            CopyColumn::Field(field) => field.name.as_str(),
            CopyColumn::RowId => "id",
        })
        .collect::<Vec<_>>();
    let expression = parameterize_columns(predicate, &names)?;
    let query = format!("SELECT ({expression})");
    let mut accepted = Vec::with_capacity(rows.len());

    for row in rows {
        if row.len() != columns.len() {
            return Err(crate::app::DiagnosticError::new(
                "22P04",
                format!(
                    "COPY WHERE row has {} columns but target expects {}",
                    row.len(),
                    columns.len()
                ),
            )
            .into());
        }
        let values = columns
            .iter()
            .zip(&row)
            .map(|(column, raw)| copy_parameter(column, raw.as_deref()))
            .collect::<Result<Vec<_>, _>>()?;
        let result = cassie.execute_sql(session, &query, values)?;
        let Some(value) = result.rows().first().and_then(|row| row.first()) else {
            continue;
        };
        if matches!(value, Value::Bool(true)) {
            accepted.push(row);
        } else if !matches!(value, Value::Bool(false) | Value::Null) {
            return Err(CassieError::Execution(
                "COPY WHERE expression did not return boolean".to_string(),
            ));
        }
    }
    Ok(accepted)
}

fn copy_parameter(column: &CopyColumn, raw: Option<&str>) -> Result<Value, CassieError> {
    let Some(raw) = raw else {
        return Ok(Value::Null);
    };
    let data_type = match column {
        CopyColumn::Field(field) => &field.data_type,
        CopyColumn::RowId => return Ok(Value::String(raw.to_owned())),
    };
    match data_type {
        DataType::SmallInt | DataType::Int | DataType::BigInt => raw
            .parse::<i64>()
            .map(Value::Int64)
            .map_err(|error| CassieError::Parse(format!("invalid COPY integer: {error}"))),
        DataType::Float => raw
            .parse::<f64>()
            .map(Value::Float64)
            .map_err(|error| CassieError::Parse(format!("invalid COPY float: {error}"))),
        DataType::Boolean => crate::types::boolean::parse_text(raw)
            .map(Value::Bool)
            .ok_or_else(|| CassieError::Parse("invalid COPY boolean".to_string())),
        _ => Ok(Value::String(raw.to_owned())),
    }
}

fn parameterize_columns(predicate: &str, columns: &[&str]) -> Result<String, CassieError> {
    let bytes = predicate.as_bytes();
    let mut output = String::with_capacity(predicate.len());
    let mut index = 0_usize;
    let mut parameter = 0_usize;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' => {
                let start = index;
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == b'\'' {
                        if bytes.get(index + 1) == Some(&b'\'') {
                            index += 2;
                            continue;
                        }
                        index += 1;
                        break;
                    }
                    index += 1;
                }
                output.push_str(&predicate[start..index]);
            }
            b'"' => {
                index += 1;
                let start = index;
                let mut name = String::new();
                while index < bytes.len() {
                    if bytes[index] == b'"' {
                        name.push_str(&predicate[start..index]);
                        if bytes.get(index + 1) == Some(&b'"') {
                            name.push('"');
                            index += 2;
                            continue;
                        }
                        index += 1;
                        break;
                    }
                    index += 1;
                }
                output.push_str(&column_parameter(columns, &name)?);
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let start = index;
                index += 1;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric()
                        || bytes[index] == b'_'
                        || bytes[index] == b'$')
                {
                    index += 1;
                }
                let name = &predicate[start..index];
                if let Some(parameter_index) = columns
                    .iter()
                    .position(|column| column.eq_ignore_ascii_case(name))
                {
                    output.push('$');
                    output.push_str(&(parameter_index + 1).to_string());
                    parameter += 1;
                } else {
                    output.push_str(name);
                }
            }
            _ => {
                let character = predicate[index..]
                    .chars()
                    .next()
                    .expect("index is within predicate");
                output.push(character);
                index += character.len_utf8();
            }
        }
    }
    if parameter == 0 {
        return Err(CassieError::Parse(
            "COPY WHERE must reference a copied column".to_string(),
        ));
    }
    Ok(output)
}

fn column_parameter(columns: &[&str], name: &str) -> Result<String, CassieError> {
    columns
        .iter()
        .position(|column| *column == name)
        .map(|index| format!("${}", index + 1))
        .ok_or_else(|| CassieError::UndefinedColumn(name.to_owned()))
}
