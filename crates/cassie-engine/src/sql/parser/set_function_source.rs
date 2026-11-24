//! FROM-only UNNEST calls, including implicit lateral scope and ordinality.

use super::super::lexical;
use super::{parse_function, relation_alias, QuerySource, SqlError};

pub(super) fn parse(raw: &str) -> Result<Option<QuerySource>, SqlError> {
    let Some(open) = raw.find('(') else {
        return Ok(None);
    };
    if !crate::sql::table_sources::is_generated(raw[..open].trim()) {
        return Ok(None);
    }
    let close = lexical::matching_paren(raw, open)
        .ok_or_else(|| SqlError::new("invalid UNNEST table-function call".into()))?;
    if crate::sql::table_sources::is_json_table(raw[..open].trim()) {
        return super::json_table_source::parse(raw, open, close).map(Some);
    }
    let function = parse_function(&raw[..=close])?
        .ok_or_else(|| SqlError::new("invalid UNNEST table-function call".into()))?;
    let tail = lexical::trim_separators(&raw[close + 1..]);
    let ordinal_end = lexical::pattern_end(tail, 0, "with ordinality");
    let with_ordinality = ordinal_end.is_some();
    let tail = ordinal_end.map_or(tail, |end| &tail[end..]);
    if let Some(record) = crate::sql::table_sources::record_source(&function.name) {
        return super::record_function_source::parse(function, record, tail, with_ordinality)
            .map(Some);
    }
    let json = crate::sql::table_sources::json_element_source(&function.name);
    let unnest = crate::sql::table_sources::is_unnest(&function.name);
    let default_name = if let Some(json) = json {
        json.function_name()
    } else if unnest {
        "unnest".to_owned()
    } else {
        "generate_series".to_owned()
    };
    let count = match json {
        Some(json) => json.column_count(),
        None if unnest => function.args.len(),
        None => 1,
    };
    let source = QuerySource::TableFunction {
        name: function.name.clone(),
        function,
        lateral: true,
        with_ordinality,
        form: None,
    };
    let aliased = relation_alias::parse(source, tail)?;
    let (source, alias, mut column_aliases, explicit_alias) = match aliased {
        QuerySource::Aliased {
            source,
            alias,
            column_aliases,
        } => (source, alias, column_aliases, true),
        source => (Box::new(source), default_name.clone(), Vec::new(), false),
    };
    for index in column_aliases.len()..count + usize::from(with_ordinality) {
        column_aliases.push(if index < count {
            if let Some(json) = json {
                json.column_name(index).to_owned()
            } else if count == 1 && explicit_alias {
                alias.clone()
            } else {
                default_name.clone()
            }
        } else {
            "ordinality".to_owned()
        });
    }
    Ok(Some(QuerySource::Aliased {
        source,
        alias,
        column_aliases,
    }))
}
