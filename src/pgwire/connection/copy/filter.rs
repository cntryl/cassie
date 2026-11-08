use crate::app::{Cassie, CassieError, CassieSession, CopyColumn};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::sql::ast::CopyStatement;
use crate::types::{DataType, Value};

#[cfg(test)]
#[path = "filter_tests.rs"]
mod tests;

fn reserve_filter_rows(
    count: usize,
    controls: &QueryExecutionControls,
) -> Result<(Vec<Vec<Option<String>>>, QueryMemoryReservation), CassieError> {
    controls.check_interruption()?;
    let base = controls.reserve_query_memory(0)?;
    crate::runtime::owned_memory::try_reserve_vec(count, &base)
}

pub(super) fn filter_rows(
    cassie: &Cassie,
    session: &CassieSession,
    statement: &CopyStatement,
    rows: Vec<Vec<Option<String>>>,
    predicate: &str,
    controls: &QueryExecutionControls,
) -> Result<(Vec<Vec<Option<String>>>, QueryMemoryReservation), CassieError> {
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
    let accepted_memory;
    let mut accepted;
    (accepted, accepted_memory) = reserve_filter_rows(rows.len(), controls)?;

    for row in rows {
        controls.check_interruption()?;
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
        let mut parameter_memory = controls.reserve_query_memory(0)?;
        let directory_memory;
        let mut values;
        (values, directory_memory) =
            crate::runtime::owned_memory::try_reserve_vec(columns.len(), &parameter_memory)?;
        parameter_memory.absorb(directory_memory);
        for (column, raw) in columns.iter().zip(&row) {
            controls.check_interruption()?;
            let (value, memory) = copy_parameter(column, raw.as_deref(), controls)?;
            values.push(value);
            parameter_memory.absorb(memory);
        }
        let result = cassie.execute_copy_where_with_controls(session, &query, values, controls)?;
        drop(parameter_memory);
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
    Ok((accepted, accepted_memory))
}

fn copy_parameter(
    column: &CopyColumn,
    raw: Option<&str>,
    controls: &QueryExecutionControls,
) -> Result<(Value, QueryMemoryReservation), CassieError> {
    controls.check_interruption()?;
    let Some(raw) = raw else {
        return Ok((Value::Null, controls.reserve_query_memory(0)?));
    };
    let data_type = match column {
        CopyColumn::Field(field) => &field.data_type,
        CopyColumn::RowId => {
            let (text, memory) = crate::app::copy_decode::copy_text(raw, controls)?;
            return Ok((Value::String(text), memory));
        }
    };
    let value = match data_type {
        DataType::SmallInt | DataType::Int | DataType::BigInt => raw
            .parse::<i64>()
            .map(Value::Int64)
            .map_err(|error| CassieError::Parse(format!("invalid COPY integer: {error}")))?,
        DataType::Float => raw
            .parse::<f64>()
            .map(Value::Float64)
            .map_err(|error| CassieError::Parse(format!("invalid COPY float: {error}")))?,
        DataType::Boolean => crate::types::boolean::parse_text(raw)
            .map(Value::Bool)
            .ok_or_else(|| CassieError::Parse("invalid COPY boolean".to_string()))?,
        _ => {
            let (text, memory) = crate::app::copy_decode::copy_text(raw, controls)?;
            return Ok((Value::String(text), memory));
        }
    };
    Ok((value, controls.reserve_query_memory(0)?))
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
                let mut start = index;
                let mut name = String::new();
                while index < bytes.len() {
                    if bytes[index] == b'"' {
                        name.push_str(&predicate[start..index]);
                        if bytes.get(index + 1) == Some(&b'"') {
                            name.push('"');
                            index += 2;
                            start = index;
                            continue;
                        }
                        index += 1;
                        break;
                    }
                    index += 1;
                }
                output.push_str(&column_parameter(columns, &name)?);
                parameter += 1;
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
