use super::{find_matching_paren, SqlError};

pub(super) fn parse_references_target(raw: &str) -> Result<(String, String), SqlError> {
    let raw = raw.trim();
    let open = raw
        .find('(')
        .ok_or_else(|| SqlError("REFERENCES requires target column list".into()))?;
    let close = find_matching_paren(raw, open)
        .ok_or_else(|| SqlError("REFERENCES requires closing ')'".into()))?;
    let table = raw[..open].trim();
    if table.is_empty() {
        return Err(SqlError("REFERENCES requires target table".into()));
    }
    let field = raw[open + 1..close].trim();
    if field.is_empty() {
        return Err(SqlError("REFERENCES requires target column".into()));
    }
    if field.split(',').count() != 1 {
        return Err(SqlError(
            "REFERENCES supports exactly one target column".into(),
        ));
    }
    Ok((table.to_string(), field.to_string()))
}
