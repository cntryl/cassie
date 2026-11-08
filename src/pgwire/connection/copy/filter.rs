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
    let base = controls.reserve_query_memory(0)?;
    let _names_memory;
    let mut names;
    (names, _names_memory) = crate::runtime::owned_memory::try_reserve_vec(columns.len(), &base)?;
    for column in &columns {
        controls.check_interruption()?;
        names.push(match column {
            CopyColumn::Field(field) => field.name.as_str(),
            CopyColumn::RowId => "id",
        });
    }
    let expression_memory;
    let expression;
    (expression, expression_memory) = parameterize_columns(predicate, &names, controls)?;
    let mut query_builder = super::filter_text::WhereText::new(controls)?;
    query_builder.push("SELECT (", controls)?;
    query_builder.push(&expression, controls)?;
    query_builder.push(")", controls)?;
    let query_memory;
    let query;
    (query, query_memory) = query_builder.into_parts();
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
    drop(query);
    drop(query_memory);
    drop(expression);
    drop(expression_memory);
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

fn parameterize_columns(
    predicate: &str,
    columns: &[&str],
    controls: &QueryExecutionControls,
) -> Result<(String, QueryMemoryReservation), CassieError> {
    let bytes = predicate.as_bytes();
    let mut output = super::filter_text::WhereText::new(controls)?;
    let mut index = 0_usize;
    let mut next_check = 0_usize;
    let mut parameter = false;
    while index < bytes.len() {
        check_scan(index, &mut next_check, controls)?;
        match bytes[index] {
            b'\'' => {
                let start = index;
                index += 1;
                while index < bytes.len() {
                    check_scan(index, &mut next_check, controls)?;
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
                output.push(&predicate[start..index], controls)?;
            }
            b'"' => {
                index += 1;
                let mut start = index;
                let mut name = super::filter_text::WhereText::new(controls)?;
                while index < bytes.len() {
                    check_scan(index, &mut next_check, controls)?;
                    if bytes[index] == b'"' {
                        name.push(&predicate[start..index], controls)?;
                        if bytes.get(index + 1) == Some(&b'"') {
                            name.push("\"", controls)?;
                            index += 2;
                            start = index;
                            continue;
                        }
                        index += 1;
                        break;
                    }
                    index += 1;
                }
                let parameter_index = find_column(columns, name.as_str(), false, controls)?
                    .ok_or_else(|| CassieError::UndefinedColumn(name.as_str().to_owned()))?;
                output.push_parameter(parameter_index, controls)?;
                parameter = true;
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let start = index;
                index += 1;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric()
                        || bytes[index] == b'_'
                        || bytes[index] == b'$')
                {
                    check_scan(index, &mut next_check, controls)?;
                    index += 1;
                }
                let name = &predicate[start..index];
                if let Some(parameter_index) = find_column(columns, name, true, controls)? {
                    output.push_parameter(parameter_index, controls)?;
                    parameter = true;
                } else {
                    output.push(name, controls)?;
                }
            }
            _ => {
                let character = predicate[index..]
                    .chars()
                    .next()
                    .expect("index is within predicate");
                let mut encoded = [0_u8; 4];
                output.push(character.encode_utf8(&mut encoded), controls)?;
                index += character.len_utf8();
            }
        }
    }
    if !parameter {
        return Err(CassieError::Parse(
            "COPY WHERE must reference a copied column".to_string(),
        ));
    }
    Ok(output.into_parts())
}

fn check_scan(
    index: usize,
    next_check: &mut usize,
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    if index >= *next_check {
        controls.check_interruption()?;
        *next_check = index.saturating_add(4096);
    }
    Ok(())
}

fn find_column(
    columns: &[&str],
    name: &str,
    insensitive: bool,
    controls: &QueryExecutionControls,
) -> Result<Option<usize>, CassieError> {
    for (index, column) in columns.iter().enumerate() {
        controls.check_interruption()?;
        if column.len() != name.len() {
            continue;
        }
        let mut equal = true;
        for (left, right) in column
            .as_bytes()
            .chunks(4096)
            .zip(name.as_bytes().chunks(4096))
        {
            controls.check_interruption()?;
            if if insensitive {
                !left.eq_ignore_ascii_case(right)
            } else {
                left != right
            } {
                equal = false;
                break;
            }
        }
        if equal {
            return Ok(Some(index));
        }
    }
    Ok(None)
}
