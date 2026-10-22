use crate::app::CassieError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CopyDirection {
    FromStdin,
    ToStdout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CopyWireFormat {
    Text,
    Csv,
    Binary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CopyTarget {
    Table {
        relation: String,
        columns: Vec<String>,
        target_sql: String,
    },
    Query(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ForceQuote {
    None,
    All,
    Columns(Vec<String>),
}

struct CopyOptions {
    format: CopyWireFormat,
    header: bool,
    delimiter: Option<char>,
    null_marker: Option<String>,
    force_quote: ForceQuote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CopyRequest {
    pub(super) target: CopyTarget,
    pub(super) direction: CopyDirection,
    pub(super) format: CopyWireFormat,
    pub(super) header: bool,
    pub(super) delimiter: Option<char>,
    pub(super) null_marker: Option<String>,
    pub(super) force_quote: ForceQuote,
}

impl CopyRequest {
    pub(super) fn table_binding_sql(&self) -> Option<String> {
        match &self.target {
            CopyTarget::Table { target_sql, .. } => Some(format!(
                "COPY {target_sql} FROM STDIN WITH (FORMAT CSV, HEADER {})",
                self.header
            )),
            CopyTarget::Query(_) => None,
        }
    }

    pub(super) fn query_sql(&self) -> String {
        match &self.target {
            CopyTarget::Query(query) => query.clone(),
            CopyTarget::Table {
                relation, columns, ..
            } => {
                let projection = if columns.is_empty() {
                    "*".to_string()
                } else {
                    columns.join(", ")
                };
                format!("SELECT {projection} FROM {relation}")
            }
        }
    }

    pub(super) fn csv_delimiter(&self) -> char {
        self.delimiter.unwrap_or(',')
    }

    pub(super) fn text_delimiter(&self) -> char {
        self.delimiter.unwrap_or('\t')
    }

    pub(super) fn null_marker(&self) -> &str {
        self.null_marker.as_deref().unwrap_or(match self.format {
            CopyWireFormat::Text => "\\N",
            CopyWireFormat::Csv | CopyWireFormat::Binary => "",
        })
    }
}

pub(super) fn parse_copy_request(sql: &str) -> Result<Option<CopyRequest>, CassieError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let Some(after_copy) = strip_keyword_prefix(trimmed, "COPY") else {
        return Ok(None);
    };
    let Some((direction_at, direction, keyword_len)) = find_transfer_clause(after_copy) else {
        return Err(CassieError::Parse(
            "COPY requires FROM STDIN or TO STDOUT".to_string(),
        ));
    };
    let target_text = after_copy[..direction_at].trim();
    let transfer = after_copy[direction_at + keyword_len..].trim();
    let target = parse_target(target_text)?;
    let expected = match direction {
        CopyDirection::FromStdin => "STDIN",
        CopyDirection::ToStdout => "STDOUT",
    };
    let Some(after_destination) = strip_keyword_prefix(transfer, expected) else {
        return Err(CassieError::Parse(format!(
            "COPY {} requires {expected}",
            match direction {
                CopyDirection::FromStdin => "FROM",
                CopyDirection::ToStdout => "TO",
            }
        )));
    };
    let options = parse_options(after_destination.trim())?;
    if direction == CopyDirection::FromStdin && options.force_quote != ForceQuote::None {
        return Err(CassieError::Parse(
            "FORCE_QUOTE is only supported for COPY TO STDOUT".to_string(),
        ));
    }
    if options.format != CopyWireFormat::Csv
        && (options.header || options.force_quote != ForceQuote::None)
    {
        return Err(CassieError::Parse(
            "HEADER and FORCE_QUOTE require FORMAT CSV".to_string(),
        ));
    }
    if options.format == CopyWireFormat::Binary
        && (options.header
            || options.delimiter.is_some()
            || options.null_marker.is_some()
            || options.force_quote != ForceQuote::None)
    {
        return Err(CassieError::Parse(
            "CSV and text options are not supported with FORMAT BINARY".to_string(),
        ));
    }
    Ok(Some(CopyRequest {
        target,
        direction,
        format: options.format,
        header: options.header,
        delimiter: options.delimiter,
        null_marker: options.null_marker,
        force_quote: options.force_quote,
    }))
}

fn parse_target(raw: &str) -> Result<CopyTarget, CassieError> {
    if raw.is_empty() {
        return Err(CassieError::Parse("COPY requires a target".to_string()));
    }
    if raw.starts_with('(') {
        let close = matching_close(raw, 0, '(', ')')
            .ok_or_else(|| CassieError::Parse("COPY query requires closing ')'".to_string()))?;
        if !raw[close + 1..].trim().is_empty() {
            return Err(CassieError::Parse(
                "COPY query target is malformed".to_string(),
            ));
        }
        let query = raw[1..close].trim();
        if query.is_empty() {
            return Err(CassieError::Parse("COPY query cannot be empty".to_string()));
        }
        return Ok(CopyTarget::Query(query.to_string()));
    }

    let open = find_unquoted(raw, '(');
    let (relation, columns, target_sql) = if let Some(open) = open {
        let close = matching_close(raw, open, '(', ')')
            .ok_or_else(|| CassieError::Parse("COPY column list requires closing ')'".into()))?;
        if !raw[close + 1..].trim().is_empty() {
            return Err(CassieError::Parse(
                "COPY table target is malformed".to_string(),
            ));
        }
        let relation = raw[..open].trim();
        if relation.is_empty() {
            return Err(CassieError::Parse("COPY requires a target table".into()));
        }
        let columns = split_top_level(&raw[open + 1..close], ',')
            .into_iter()
            .map(str::trim)
            .map(str::to_string)
            .collect::<Vec<_>>();
        if columns.is_empty() || columns.iter().any(String::is_empty) {
            return Err(CassieError::Parse(
                "COPY column list cannot be empty".into(),
            ));
        }
        (relation.to_string(), columns, raw.trim().to_string())
    } else {
        let relation = raw.trim();
        (relation.to_string(), Vec::new(), relation.to_string())
    };
    Ok(CopyTarget::Table {
        relation,
        columns,
        target_sql,
    })
}

fn find_transfer_clause(sql: &str) -> Option<(usize, CopyDirection, usize)> {
    let bytes = sql.as_bytes();
    let mut depth = 0_usize;
    let mut quote = None;
    let mut index = 0_usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(quote_byte) = quote {
            if byte == quote_byte {
                if bytes.get(index + 1) == Some(&quote_byte) {
                    index += 2;
                    continue;
                }
                quote = None;
            }
            index += 1;
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'(' => depth = depth.saturating_add(1),
            b')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => {
                for (word, direction) in [
                    ("FROM", CopyDirection::FromStdin),
                    ("TO", CopyDirection::ToStdout),
                ] {
                    if keyword_at(bytes, index, word) {
                        return Some((index, direction, word.len()));
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn keyword_at(bytes: &[u8], index: usize, keyword: &str) -> bool {
    let end = index.saturating_add(keyword.len());
    index > 0
        && bytes[index - 1].is_ascii_whitespace()
        && bytes
            .get(index..end)
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(keyword.as_bytes()))
        && bytes.get(end).is_some_and(u8::is_ascii_whitespace)
}

fn parse_options(raw: &str) -> Result<CopyOptions, CassieError> {
    if raw.is_empty() {
        return Ok(CopyOptions {
            format: CopyWireFormat::Text,
            header: false,
            delimiter: None,
            null_marker: None,
            force_quote: ForceQuote::None,
        });
    }
    let Some(options) = strip_keyword_prefix(raw, "WITH") else {
        return Err(CassieError::Parse("COPY options require WITH".to_string()));
    };
    let options = options.trim();
    let items = if options.starts_with('(') {
        let close = matching_close(options, 0, '(', ')')
            .ok_or_else(|| CassieError::Parse("COPY options require closing ')'".into()))?;
        if !options[close + 1..].trim().is_empty() {
            return Err(CassieError::Parse("COPY options are malformed".into()));
        }
        split_top_level(&options[1..close], ',')
    } else {
        options.split_ascii_whitespace().collect()
    };

    let mut parsed = CopyOptions {
        format: CopyWireFormat::Csv,
        header: false,
        delimiter: None,
        null_marker: None,
        force_quote: ForceQuote::None,
    };
    let mut seen = std::collections::HashSet::new();
    for item in items {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        parse_option_item(item, &mut parsed, &mut seen)?;
    }

    Ok(parsed)
}

fn parse_option_item(
    item: &str,
    options: &mut CopyOptions,
    seen: &mut std::collections::HashSet<String>,
) -> Result<(), CassieError> {
    let (key, value) = split_option(item);
    let key = key.to_ascii_uppercase();
    if !seen.insert(key.clone()) {
        return Err(CassieError::Parse(format!(
            "COPY option '{key}' is specified more than once"
        )));
    }
    match key.as_str() {
        "FORMAT" => {
            options.format =
                parse_format(value.ok_or_else(|| {
                    CassieError::Parse("COPY FORMAT requires a value".to_string())
                })?)?;
        }
        "CSV" | "TEXT" | "BINARY" if value.is_none() => {
            options.format = parse_format(&key)?;
        }
        "HEADER" => options.header = value.map_or(Ok(true), parse_bool)?,
        "DELIMITER" => options.delimiter = Some(parse_delimiter(value)?),
        "NULL" => {
            options.null_marker =
                Some(parse_string_literal(value.ok_or_else(|| {
                    CassieError::Parse("COPY NULL requires a string".to_string())
                })?)?);
        }
        "FORCE_QUOTE" => options.force_quote = parse_force_quote(value)?,
        _ => {
            return Err(CassieError::Unsupported(format!(
                "unsupported COPY option '{item}'"
            )));
        }
    }
    Ok(())
}

fn parse_delimiter(value: Option<&str>) -> Result<char, CassieError> {
    let literal = parse_string_literal(
        value.ok_or_else(|| CassieError::Parse("COPY DELIMITER requires a string".to_string()))?,
    )?;
    let mut characters = literal.chars();
    let Some(character) = characters.next() else {
        return Err(CassieError::Parse(
            "COPY DELIMITER must be one character".to_string(),
        ));
    };
    if characters.next().is_some() {
        return Err(CassieError::Parse(
            "COPY DELIMITER must be one character".to_string(),
        ));
    }
    Ok(character)
}

fn parse_force_quote(value: Option<&str>) -> Result<ForceQuote, CassieError> {
    let value = value.ok_or_else(|| {
        CassieError::Parse("COPY FORCE_QUOTE requires '*' or a column list".into())
    })?;
    if value == "*" {
        return Ok(ForceQuote::All);
    }
    let inner = value
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
        .ok_or_else(|| CassieError::Parse("COPY FORCE_QUOTE is malformed".into()))?;
    let columns = split_top_level(inner, ',')
        .into_iter()
        .map(|column| normalize_identifier(column.trim()))
        .collect::<Result<Vec<_>, _>>()?;
    if columns.is_empty() || columns.iter().any(String::is_empty) {
        return Err(CassieError::Parse(
            "COPY FORCE_QUOTE column list cannot be empty".into(),
        ));
    }
    Ok(ForceQuote::Columns(columns))
}

fn parse_format(raw: &str) -> Result<CopyWireFormat, CassieError> {
    match raw.trim().trim_matches('"').to_ascii_uppercase().as_str() {
        "TEXT" => Ok(CopyWireFormat::Text),
        "CSV" => Ok(CopyWireFormat::Csv),
        "BINARY" => Ok(CopyWireFormat::Binary),
        _ => Err(CassieError::Unsupported(format!(
            "unsupported COPY format '{raw}'"
        ))),
    }
}

fn split_option(raw: &str) -> (&str, Option<&str>) {
    let bytes = raw.as_bytes();
    let mut quote = None;
    let mut depth = 0_usize;
    let mut index = 0_usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(quote_byte) = quote {
            if byte == quote_byte {
                if bytes.get(index + 1) == Some(&quote_byte) {
                    index += 2;
                    continue;
                }
                quote = None;
            }
            index += 1;
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'(' => depth = depth.saturating_add(1),
            b')' => depth = depth.saturating_sub(1),
            byte if byte.is_ascii_whitespace() && depth == 0 => {
                return (raw[..index].trim(), Some(raw[index..].trim()));
            }
            _ => {}
        }
        index += 1;
    }
    (raw.trim(), None)
}

fn split_top_level(raw: &str, delimiter: char) -> Vec<&str> {
    let bytes = raw.as_bytes();
    let delimiter = u8::try_from(delimiter).ok();
    let mut values = Vec::new();
    let mut start = 0_usize;
    let mut depth = 0_usize;
    let mut quote = None;
    let mut index = 0_usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(quote_byte) = quote {
            if byte == quote_byte {
                if bytes.get(index + 1) == Some(&quote_byte) {
                    index += 2;
                    continue;
                }
                quote = None;
            }
        } else {
            match byte {
                b'\'' | b'"' => quote = Some(byte),
                b'(' => depth = depth.saturating_add(1),
                b')' => depth = depth.saturating_sub(1),
                _ if Some(byte) == delimiter && depth == 0 => {
                    values.push(&raw[start..index]);
                    start = index + 1;
                }
                _ => {}
            }
        }
        index += 1;
    }
    values.push(&raw[start..]);
    values
}

fn find_unquoted(raw: &str, sought: char) -> Option<usize> {
    let mut quote = None;
    for (index, byte) in raw.bytes().enumerate() {
        if let Some(quote_byte) = quote {
            if byte == quote_byte {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
        } else if char::from(byte) == sought {
            return Some(index);
        }
    }
    None
}

fn matching_close(raw: &str, open: usize, left: char, right: char) -> Option<usize> {
    let bytes = raw.as_bytes();
    let left = u8::try_from(left).ok()?;
    let right = u8::try_from(right).ok()?;
    let mut depth = 0_usize;
    let mut quote = None;
    let mut index = open;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(quote_byte) = quote {
            if byte == quote_byte {
                if bytes.get(index + 1) == Some(&quote_byte) {
                    index += 2;
                    continue;
                }
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
        } else if byte == left {
            depth = depth.saturating_add(1);
        } else if byte == right {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

fn strip_keyword_prefix<'a>(raw: &'a str, keyword: &str) -> Option<&'a str> {
    let prefix = raw.get(..keyword.len())?;
    if !prefix.eq_ignore_ascii_case(keyword)
        || raw
            .as_bytes()
            .get(keyword.len())
            .is_some_and(|byte| !byte.is_ascii_whitespace())
    {
        return None;
    }
    Some(raw[keyword.len()..].trim())
}

fn parse_bool(raw: &str) -> Result<bool, CassieError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "on" | "1" => Ok(true),
        "false" | "off" | "0" => Ok(false),
        _ => Err(CassieError::Parse(format!(
            "invalid COPY HEADER value '{raw}'"
        ))),
    }
}

fn parse_string_literal(raw: &str) -> Result<String, CassieError> {
    let raw = raw.trim();
    if !raw.starts_with('\'') || !raw.ends_with('\'') || raw.len() < 2 {
        return Err(CassieError::Parse(
            "COPY option value must be a single-quoted string".to_string(),
        ));
    }
    let inner = &raw[1..raw.len() - 1];
    Ok(inner.replace("''", "'"))
}

fn normalize_identifier(raw: &str) -> Result<String, CassieError> {
    let raw = raw.trim();
    if raw.starts_with('"') {
        if !raw.ends_with('"') || raw.len() < 2 {
            return Err(CassieError::Parse(
                "COPY option contains malformed quoted identifier".to_string(),
            ));
        }
        Ok(raw[1..raw.len() - 1].replace("\"\"", "\""))
    } else {
        Ok(raw.to_ascii_lowercase())
    }
}
