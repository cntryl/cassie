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
    let function = parse_function(&raw[..=close])?
        .ok_or_else(|| SqlError::new("invalid UNNEST table-function call".into()))?;
    let tail = lexical::trim_separators(&raw[close + 1..]);
    let ordinal_end = lexical::pattern_end(tail, 0, "with ordinality");
    let with_ordinality = ordinal_end.is_some();
    let tail = ordinal_end.map_or(tail, |end| &tail[end..]);
    let unnest = crate::sql::table_sources::is_unnest(&function.name);
    let default_name = if unnest { "unnest" } else { "generate_series" };
    let count = if unnest { function.args.len() } else { 1 };
    let source = QuerySource::TableFunction {
        name: function.name.clone(),
        function,
        lateral: true,
        with_ordinality,
    };
    let aliased = relation_alias::parse(source, tail)?;
    let (source, alias, mut column_aliases, explicit_alias) = match aliased {
        QuerySource::Aliased {
            source,
            alias,
            column_aliases,
        } => (source, alias, column_aliases, true),
        source => (Box::new(source), default_name.to_owned(), Vec::new(), false),
    };
    for index in column_aliases.len()..count + usize::from(with_ordinality) {
        column_aliases.push(if index < count {
            if count == 1 && explicit_alias {
                alias.clone()
            } else {
                default_name.to_owned()
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
