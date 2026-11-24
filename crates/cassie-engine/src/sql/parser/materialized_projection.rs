use super::{
    parse_index_options, CreateMaterializedProjectionStatement,
    DropMaterializedProjectionStatement, ParsedStatement, QueryStatement, SqlError,
    VerifyProjectionStatement,
};

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
        .ok_or_else(|| SqlError::new("CREATE MATERIALIZED PROJECTION requires AS clause".into()))?;
    let name_part = rest[..as_pos].trim();
    let lower_name_part = name_part.to_ascii_lowercase();
    let (name, options) = if let Some(with_pos) = lower_name_part.find(" with ") {
        let name = name_part[..with_pos].trim();
        let options_raw = name_part[with_pos + 1..].trim();
        let (options, remainder) = parse_index_options(options_raw)?;
        if !remainder.is_empty() {
            return Err(SqlError::new(
                "unsupported CREATE MATERIALIZED PROJECTION options".into(),
            ));
        }
        (name, options)
    } else {
        (name_part, std::collections::BTreeMap::default())
    };
    if name.is_empty() {
        return Err(SqlError::new(
            "CREATE MATERIALIZED PROJECTION requires a name".into(),
        ));
    }
    if name.split_whitespace().count() != 1 {
        return Err(SqlError::new(
            "CREATE MATERIALIZED PROJECTION supports only one projection name".into(),
        ));
    }

    let query = rest[as_pos + 4..].trim();
    if query.is_empty() {
        return Err(SqlError::new(
            "CREATE MATERIALIZED PROJECTION requires a query body".into(),
        ));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CreateMaterializedProjection(
            CreateMaterializedProjectionStatement {
                name: name.to_ascii_lowercase(),
                if_not_exists,
                options,
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
        return Err(SqlError::new(
            "REFRESH MATERIALIZED PROJECTION requires a name".into(),
        ));
    }
    if name.split_whitespace().count() != 1 {
        return Err(SqlError::new(
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
        return Err(SqlError::new(
            "DROP MATERIALIZED PROJECTION requires a name".into(),
        ));
    }
    if rest.split_whitespace().count() != 1 {
        return Err(SqlError::new(
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
        return Err(SqlError::new(
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
            return Err(SqlError::new(
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
        return Err(SqlError::new(
            "DROP MATERIALIZED PROJECTION VERSION requires name VERSION version_id".into(),
        ));
    };
    if !version_keyword.eq_ignore_ascii_case("version") {
        return Err(SqlError::new(
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
        return Err(SqlError::new("VERIFY PROJECTION requires a name".into()));
    }
    let name = tokens[0].to_ascii_lowercase();
    let mut version_id = None;
    let mut mode = crate::sql::ast::ProjectionVerificationMode::Full;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].to_ascii_lowercase().as_str() {
            "version" => {
                let Some(value) = tokens.get(index + 1) else {
                    return Err(SqlError::new(
                        "VERIFY PROJECTION VERSION requires a version id".into(),
                    ));
                };
                version_id = Some((*value).to_string());
                index += 2;
            }
            "mode" => {
                let Some(value) = tokens.get(index + 1) else {
                    return Err(SqlError::new(
                        "VERIFY PROJECTION MODE requires a value".into(),
                    ));
                };
                mode = parse_projection_verification_mode(value)?;
                index += 2;
            }
            _ => {
                return Err(SqlError::new(
                    "unsupported VERIFY PROJECTION option".to_string(),
                ));
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

pub(super) fn parse_diff_projection_statement(trimmed: &str) -> Result<ParsedStatement, SqlError> {
    let prefix = "DIFF PROJECTION";
    let rest = trimmed[prefix.len()..].trim().trim_end_matches(';').trim();
    let tokens = rest.split_whitespace().collect::<Vec<_>>();
    if tokens.is_empty() {
        return Err(SqlError::new(
            "DIFF PROJECTION requires a left target".into(),
        ));
    }
    let mut index = 0;
    let left = parse_projection_diff_target(tokens.as_slice(), &mut index, "DIFF PROJECTION")?;
    if tokens
        .get(index)
        .is_none_or(|token| !token.eq_ignore_ascii_case("with"))
    {
        return Err(SqlError::new("DIFF PROJECTION requires WITH target".into()));
    }
    index += 1;
    let right = parse_projection_diff_target(tokens.as_slice(), &mut index, "DIFF PROJECTION")?;
    let mut limit = None;
    let mut after = None;
    while index < tokens.len() {
        match tokens[index].to_ascii_lowercase().as_str() {
            "limit" => {
                let Some(value) = tokens.get(index + 1) else {
                    return Err(SqlError::new(
                        "DIFF PROJECTION LIMIT requires a value".into(),
                    ));
                };
                limit = Some(value.parse::<usize>().map_err(|_| {
                    SqlError::new("DIFF PROJECTION LIMIT must be a positive integer".into())
                })?);
                index += 2;
            }
            "after" => {
                let Some(value) = tokens.get(index + 1) else {
                    return Err(SqlError::new(
                        "DIFF PROJECTION AFTER requires a cursor".into(),
                    ));
                };
                after = Some((*value).to_string());
                index += 2;
            }
            _ => return Err(SqlError::new("unsupported DIFF PROJECTION option".into())),
        }
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::DiffProjection(crate::sql::ast::DiffProjectionStatement {
            left,
            right,
            limit,
            after,
        }),
    })
}

pub(super) fn parse_compare_projection_statement(
    trimmed: &str,
) -> Result<ParsedStatement, SqlError> {
    let prefix = "COMPARE PROJECTION";
    let rest = trimmed[prefix.len()..].trim().trim_end_matches(';').trim();
    let lower = rest.to_ascii_lowercase();
    let Some(with_pos) = lower.find(" with manifest ") else {
        return Err(SqlError::new(
            "COMPARE PROJECTION requires WITH MANIFEST '<json>'".into(),
        ));
    };
    let mut target_tokens = rest[..with_pos].split_whitespace().collect::<Vec<_>>();
    let mut index = 0;
    let target =
        parse_projection_diff_target(target_tokens.as_slice(), &mut index, "COMPARE PROJECTION")?;
    if index != target_tokens.len() {
        return Err(SqlError::new(
            "unsupported COMPARE PROJECTION target option".into(),
        ));
    }
    let manifest = rest[with_pos + " with manifest ".len()..].trim();
    if manifest.is_empty() {
        return Err(SqlError::new(
            "COMPARE PROJECTION MANIFEST requires a value".into(),
        ));
    }
    let manifest = manifest.trim_matches('\'').trim_matches('"').to_string();
    target_tokens.clear();

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CompareProjection(crate::sql::ast::CompareProjectionStatement {
            target,
            manifest,
        }),
    })
}

pub(super) fn parse_plan_repair_projection_statement(
    trimmed: &str,
) -> Result<ParsedStatement, SqlError> {
    let statement = parse_projection_repair_statement(trimmed, "PLAN REPAIR PROJECTION")?;
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::PlanRepairProjection(
            crate::sql::ast::PlanRepairProjectionStatement {
                target: statement.target,
                scope: statement.scope,
            },
        ),
    })
}

pub(super) fn parse_repair_projection_statement(
    trimmed: &str,
) -> Result<ParsedStatement, SqlError> {
    let statement = parse_projection_repair_statement(trimmed, "REPAIR PROJECTION")?;
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::RepairProjection(crate::sql::ast::RepairProjectionStatement {
            target: statement.target,
            scope: statement.scope,
        }),
    })
}

struct ParsedProjectionRepair {
    target: crate::sql::ast::ProjectionDiffTarget,
    scope: crate::sql::ast::ProjectionRepairScope,
}

fn parse_projection_repair_statement(
    trimmed: &str,
    command: &str,
) -> Result<ParsedProjectionRepair, SqlError> {
    let rest = trimmed[command.len()..].trim().trim_end_matches(';').trim();
    let tokens = rest.split_whitespace().collect::<Vec<_>>();
    let mut index = 0;
    let target = parse_projection_diff_target(tokens.as_slice(), &mut index, command)?;
    let Some(scope_keyword) = tokens.get(index) else {
        return Err(SqlError::new(format!("{command} requires SCOPE")));
    };
    if !scope_keyword.eq_ignore_ascii_case("scope") {
        return Err(SqlError::new(format!("{command} requires SCOPE")));
    }
    let Some(scope) = tokens.get(index + 1) else {
        return Err(SqlError::new(format!("{command} SCOPE requires a value")));
    };
    index += 2;
    if index != tokens.len() {
        return Err(SqlError::new(format!("unsupported {command} option")));
    }
    Ok(ParsedProjectionRepair {
        target,
        scope: parse_projection_repair_scope(scope)?,
    })
}

fn parse_projection_diff_target(
    tokens: &[&str],
    index: &mut usize,
    command: &str,
) -> Result<crate::sql::ast::ProjectionDiffTarget, SqlError> {
    let Some(name) = tokens.get(*index) else {
        return Err(SqlError::new(format!(
            "{command} requires a projection name"
        )));
    };
    let mut target = crate::sql::ast::ProjectionDiffTarget {
        name: name.to_ascii_lowercase(),
        version_id: None,
    };
    *index += 1;
    if tokens
        .get(*index)
        .is_some_and(|token| token.eq_ignore_ascii_case("version"))
    {
        let Some(version_id) = tokens.get(*index + 1) else {
            return Err(SqlError::new(format!("{command} VERSION requires an id")));
        };
        target.version_id = Some((*version_id).to_string());
        *index += 2;
    }
    Ok(target)
}

fn parse_projection_repair_scope(
    raw: &str,
) -> Result<crate::sql::ast::ProjectionRepairScope, SqlError> {
    match raw.to_ascii_lowercase().replace('-', "_").as_str() {
        "row" => Ok(crate::sql::ast::ProjectionRepairScope::Row),
        "range" => Ok(crate::sql::ast::ProjectionRepairScope::Range),
        "index" => Ok(crate::sql::ast::ProjectionRepairScope::Index),
        "projection_version" => Ok(crate::sql::ast::ProjectionRepairScope::ProjectionVersion),
        "full_rebuild" => Ok(crate::sql::ast::ProjectionRepairScope::FullRebuild),
        _ => Err(SqlError::new(format!(
            "unsupported projection repair scope '{raw}'"
        ))),
    }
}

fn parse_projection_verification_mode(
    raw: &str,
) -> Result<crate::sql::ast::ProjectionVerificationMode, SqlError> {
    match raw.to_ascii_lowercase().replace('-', "_").as_str() {
        "metadata_only" => Ok(crate::sql::ast::ProjectionVerificationMode::MetadataOnly),
        "hashes_only" => Ok(crate::sql::ast::ProjectionVerificationMode::HashesOnly),
        "indexes_only" => Ok(crate::sql::ast::ProjectionVerificationMode::IndexesOnly),
        "full" => Ok(crate::sql::ast::ProjectionVerificationMode::Full),
        _ => Err(SqlError::new(format!(
            "unsupported VERIFY PROJECTION mode '{raw}'"
        ))),
    }
}
