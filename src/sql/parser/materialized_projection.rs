use super::*;

pub(super) fn parse_create_materialized_projection_statement(
    trimmed: &str,
) -> Result<ParsedStatement, SqlError> {
    let prefix = "CREATE MATERIALIZED PROJECTION";
    let mut rest = trimmed[prefix.len()..].trim().trim_end_matches(';').trim();
    let mut if_not_exists = false;

    if rest.to_ascii_lowercase().starts_with("if not exists ") {
        if_not_exists = true;
        rest = rest["IF NOT EXISTS".len()..].trim();
    }

    let lower = rest.to_ascii_lowercase();
    let as_pos = lower
        .find(" as ")
        .ok_or_else(|| SqlError("CREATE MATERIALIZED PROJECTION requires AS clause".into()))?;
    let name = rest[..as_pos].trim();
    if name.is_empty() {
        return Err(SqlError(
            "CREATE MATERIALIZED PROJECTION requires a name".into(),
        ));
    }
    if name.split_whitespace().count() != 1 {
        return Err(SqlError(
            "CREATE MATERIALIZED PROJECTION supports only one projection name".into(),
        ));
    }

    let query = rest[as_pos + 4..].trim();
    if query.is_empty() {
        return Err(SqlError(
            "CREATE MATERIALIZED PROJECTION requires a query body".into(),
        ));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CreateMaterializedProjection(
            CreateMaterializedProjectionStatement {
                name: name.to_ascii_lowercase(),
                if_not_exists,
                query: query.to_string(),
            },
        ),
    })
}

pub(super) fn parse_refresh_materialized_projection_statement(
    trimmed: &str,
) -> Result<ParsedStatement, SqlError> {
    let prefix = "REFRESH MATERIALIZED PROJECTION";
    let name = trimmed[prefix.len()..].trim().trim_end_matches(';').trim();
    if name.is_empty() {
        return Err(SqlError(
            "REFRESH MATERIALIZED PROJECTION requires a name".into(),
        ));
    }
    if name.split_whitespace().count() != 1 {
        return Err(SqlError(
            "REFRESH MATERIALIZED PROJECTION supports only one projection name".into(),
        ));
    }
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::RefreshMaterializedProjection(
            crate::sql::ast::RefreshMaterializedProjectionStatement {
                name: name.to_ascii_lowercase(),
            },
        ),
    })
}

pub(super) fn parse_drop_materialized_projection_statement(
    trimmed: &str,
) -> Result<ParsedStatement, SqlError> {
    let prefix = "DROP MATERIALIZED PROJECTION";
    let mut rest = trimmed[prefix.len()..].trim().trim_end_matches(';').trim();
    let mut if_exists = false;
    if rest.to_ascii_lowercase().starts_with("if exists ") {
        if_exists = true;
        rest = rest["IF EXISTS".len()..].trim();
    }
    if rest.is_empty() {
        return Err(SqlError(
            "DROP MATERIALIZED PROJECTION requires a name".into(),
        ));
    }
    if rest.split_whitespace().count() != 1 {
        return Err(SqlError(
            "DROP MATERIALIZED PROJECTION supports only one projection name".into(),
        ));
    }
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::DropMaterializedProjection(
            DropMaterializedProjectionStatement {
                name: rest.to_ascii_lowercase(),
                if_exists,
            },
        ),
    })
}

pub(super) fn parse_alter_materialized_projection_statement(
    trimmed: &str,
) -> Result<ParsedStatement, SqlError> {
    let prefix = "ALTER MATERIALIZED PROJECTION";
    let rest = trimmed[prefix.len()..].trim().trim_end_matches(';').trim();
    let tokens = rest.split_whitespace().collect::<Vec<_>>();
    if tokens.len() < 3 {
        return Err(SqlError(
            "ALTER MATERIALIZED PROJECTION requires an operation".into(),
        ));
    }
    let name = tokens[0].to_ascii_lowercase();
    let operation = match &tokens[1..] {
        [build, version]
            if build.eq_ignore_ascii_case("build") && version.eq_ignore_ascii_case("version") =>
        {
            crate::sql::ast::AlterMaterializedProjectionOperation::BuildVersion
        }
        [activate, version, version_id]
            if activate.eq_ignore_ascii_case("activate")
                && version.eq_ignore_ascii_case("version") =>
        {
            crate::sql::ast::AlterMaterializedProjectionOperation::ActivateVersion {
                version_id: (*version_id).to_string(),
                unsafe_override: false,
            }
        }
        [activate, version, version_id, unsafe_token]
            if activate.eq_ignore_ascii_case("activate")
                && version.eq_ignore_ascii_case("version")
                && unsafe_token.eq_ignore_ascii_case("unsafe") =>
        {
            crate::sql::ast::AlterMaterializedProjectionOperation::ActivateVersion {
                version_id: (*version_id).to_string(),
                unsafe_override: true,
            }
        }
        _ => {
            return Err(SqlError(
                "unsupported ALTER MATERIALIZED PROJECTION operation".into(),
            ));
        }
    };

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::AlterMaterializedProjection(
            crate::sql::ast::AlterMaterializedProjectionStatement { name, operation },
        ),
    })
}

pub(super) fn parse_drop_materialized_projection_version_statement(
    trimmed: &str,
) -> Result<ParsedStatement, SqlError> {
    let prefix = "DROP MATERIALIZED PROJECTION VERSION";
    let rest = trimmed[prefix.len()..].trim().trim_end_matches(';').trim();
    let tokens = rest.split_whitespace().collect::<Vec<_>>();
    let [name, version_keyword, version_id] = tokens.as_slice() else {
        return Err(SqlError(
            "DROP MATERIALIZED PROJECTION VERSION requires name VERSION version_id".into(),
        ));
    };
    if !version_keyword.eq_ignore_ascii_case("version") {
        return Err(SqlError(
            "DROP MATERIALIZED PROJECTION VERSION requires VERSION keyword".into(),
        ));
    }
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::DropMaterializedProjectionVersion(
            crate::sql::ast::DropMaterializedProjectionVersionStatement {
                name: name.to_ascii_lowercase(),
                version_id: (*version_id).to_string(),
            },
        ),
    })
}

pub(super) fn parse_verify_projection_statement(
    trimmed: &str,
) -> Result<ParsedStatement, SqlError> {
    let prefix = "VERIFY PROJECTION";
    let rest = trimmed[prefix.len()..].trim().trim_end_matches(';').trim();
    let tokens = rest.split_whitespace().collect::<Vec<_>>();
    if tokens.is_empty() {
        return Err(SqlError("VERIFY PROJECTION requires a name".into()));
    }
    let name = tokens[0].to_ascii_lowercase();
    let mut version_id = None;
    let mut mode = crate::sql::ast::ProjectionVerificationMode::Full;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].to_ascii_lowercase().as_str() {
            "version" => {
                let Some(value) = tokens.get(index + 1) else {
                    return Err(SqlError(
                        "VERIFY PROJECTION VERSION requires a version id".into(),
                    ));
                };
                version_id = Some((*value).to_string());
                index += 2;
            }
            "mode" => {
                let Some(value) = tokens.get(index + 1) else {
                    return Err(SqlError("VERIFY PROJECTION MODE requires a value".into()));
                };
                mode = parse_projection_verification_mode(value)?;
                index += 2;
            }
            _ => {
                return Err(SqlError("unsupported VERIFY PROJECTION option".to_string()));
            }
        }
    }
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::VerifyProjection(VerifyProjectionStatement {
            name,
            version_id,
            mode,
        }),
    })
}

fn parse_projection_verification_mode(
    raw: &str,
) -> Result<crate::sql::ast::ProjectionVerificationMode, SqlError> {
    match raw.to_ascii_lowercase().replace('-', "_").as_str() {
        "metadata_only" => Ok(crate::sql::ast::ProjectionVerificationMode::MetadataOnly),
        "hashes_only" => Ok(crate::sql::ast::ProjectionVerificationMode::HashesOnly),
        "indexes_only" => Ok(crate::sql::ast::ProjectionVerificationMode::IndexesOnly),
        "full" => Ok(crate::sql::ast::ProjectionVerificationMode::Full),
        _ => Err(SqlError(format!(
            "unsupported VERIFY PROJECTION mode '{raw}'"
        ))),
    }
}
