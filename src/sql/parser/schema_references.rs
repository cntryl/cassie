use super::schema_identifiers::{parse_identifier, parse_identifier_list};
use super::{find_matching_paren, SqlError};

pub(super) fn parse_references_target(raw: &str) -> Result<(String, String), SqlError> {
    let (table, field, _) = parse_references_target_with_rest(raw)?;
    Ok((table, field))
}

pub(super) fn parse_references_target_with_rest(
    raw: &str,
) -> Result<(String, String, &str), SqlError> {
    let raw = raw.trim();
    let open = raw
        .find('(')
        .ok_or_else(|| SqlError("REFERENCES requires target column list".into()))?;
    let close = find_matching_paren(raw, open)
        .ok_or_else(|| SqlError("REFERENCES requires closing ')'".into()))?;
    let table = parse_identifier(raw[..open].trim())?;
    if table.is_empty() {
        return Err(SqlError("REFERENCES requires target table".into()));
    }
    let fields = parse_identifier_list(raw[open + 1..close].trim())?;
    if fields.is_empty() || fields.iter().any(|field| field.trim().is_empty()) {
        return Err(SqlError("REFERENCES requires target column".into()));
    }
    if fields.len() != 1 {
        return Err(SqlError(
            "REFERENCES supports exactly one target column".into(),
        ));
    }
    Ok((table, fields[0].clone(), raw[close + 1..].trim_start()))
}
