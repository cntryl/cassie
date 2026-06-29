use super::expr::split_csv;
use super::{ParsedStatement, SqlError, QueryStatement};

pub(super) fn parse_copy_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("copy ") {
        return Err(SqlError("COPY requires a target table".into()));
    }

    let after_copy = trimmed[4..].trim();
    let lower_after_copy = after_copy.to_ascii_lowercase();
    let from_pos = lower_after_copy
        .find(" from ")
        .ok_or_else(|| SqlError("COPY requires FROM STDIN".into()))?;
    let target = after_copy[..from_pos].trim();
    let source = after_copy[(from_pos + 6)..].trim();

    let (table, columns) = parse_copy_target(target)?;
    let (stdin, options) = split_copy_source(source);
    if !stdin.eq_ignore_ascii_case("stdin") {
        return Err(SqlError("COPY only supports FROM STDIN".into()));
    }

    let (format, header) = parse_copy_options(options)?;
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::Copy(crate::sql::ast::CopyStatement {
            table,
            columns,
            format,
            header,
        }),
    })
}

fn parse_copy_target(raw: &str) -> Result<(String, Vec<String>), SqlError> {
    if raw.is_empty() {
        return Err(SqlError("COPY requires a target table".into()));
    }

    let Some(open) = raw.find('(') else {
        return Ok((raw.trim().to_string(), Vec::new()));
    };
    let close = raw
        .rfind(')')
        .ok_or_else(|| SqlError("COPY column list requires closing ')'".into()))?;
    if close < open || !raw[(close + 1)..].trim().is_empty() {
        return Err(SqlError("COPY column list is malformed".into()));
    }
    let table = raw[..open].trim();
    if table.is_empty() {
        return Err(SqlError("COPY requires a target table".into()));
    }
    let columns_raw = raw[(open + 1)..close].trim();
    if columns_raw.is_empty() {
        return Err(SqlError("COPY column list cannot be empty".into()));
    }

    let columns = split_csv(columns_raw)
        .into_iter()
        .map(|column| column.trim().to_string())
        .collect::<Vec<_>>();
    if columns.iter().any(std::string::String::is_empty) {
        return Err(SqlError(
            "COPY column list cannot include empty columns".into(),
        ));
    }

    Ok((table.to_string(), columns))
}

fn split_copy_source(raw: &str) -> (&str, Option<&str>) {
    let lower = raw.to_ascii_lowercase();
    if let Some(with_pos) = lower.find(" with ") {
        return (raw[..with_pos].trim(), Some(raw[(with_pos + 6)..].trim()));
    }
    (raw.trim(), None)
}

fn parse_copy_options(
    options: Option<&str>,
) -> Result<(crate::sql::ast::CopyFormat, bool), SqlError> {
    let Some(options) = options else {
        return Ok((crate::sql::ast::CopyFormat::Csv, false));
    };
    let options = options.trim();
    if !options.starts_with('(') || !options.ends_with(')') {
        return Err(SqlError("COPY WITH options require parentheses".into()));
    }

    let mut format = crate::sql::ast::CopyFormat::Csv;
    let mut header = false;
    for option in split_csv(&options[1..(options.len() - 1)]) {
        let option = option.trim();
        if option.is_empty() {
            continue;
        }
        let parts = option.split_whitespace().collect::<Vec<_>>();
        match parts.as_slice() {
            [key, value] if key.eq_ignore_ascii_case("format") => {
                if !value.eq_ignore_ascii_case("csv") {
                    return Err(SqlError("COPY only supports FORMAT csv".into()));
                }
                format = crate::sql::ast::CopyFormat::Csv;
            }
            [key, value] if key.eq_ignore_ascii_case("header") => {
                header = parse_copy_bool(value)?;
            }
            [key] if key.eq_ignore_ascii_case("header") => {
                header = true;
            }
            _ => {
                return Err(SqlError(format!("unsupported COPY option '{option}'")));
            }
        }
    }

    Ok((format, header))
}

fn parse_copy_bool(raw: &str) -> Result<bool, SqlError> {
    match raw.to_ascii_lowercase().as_str() {
        "true" | "on" | "1" => Ok(true),
        "false" | "off" | "0" => Ok(false),
        _ => Err(SqlError(format!("invalid COPY HEADER value '{raw}'"))),
    }
}
