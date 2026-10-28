use super::schema_fields::apply_default_constraint;
use super::schema_identifiers::{parse_column_reference, parse_identifier};
use super::sequence_options::{parse_sequence_clauses, SequenceClauseRules};
use super::{
    parse_if_exists, parse_if_not_exists, split_first_token, AlterSequenceStatement,
    AlterTableOperation, CreateSequenceStatement, DataType, DropSequenceStatement, FieldConstraint,
    ParsedStatement, QueryStatement, SqlError,
};
use crate::catalog::SequencePersistence;

pub(super) fn parse_alter_column_operation(raw: &str) -> Result<AlterTableOperation, SqlError> {
    let (field, action) = split_first_token(raw)
        .ok_or_else(|| SqlError::new("ALTER COLUMN requires a column name".into()))?;
    let field = parse_column_reference(&field)?;
    let action = action.trim();
    let lower = action.to_ascii_lowercase();

    let type_clause = lower
        .strip_prefix("set data type ")
        .map(|_| "set data type ".len())
        .or_else(|| lower.strip_prefix("type ").map(|_| "type ".len()));
    if let Some(type_offset) = type_clause {
        let clause = action[type_offset..].trim();
        let using_offset = super::find_top_level_keyword(clause, 0, "using");
        let Some(using_offset) = using_offset else {
            return Err(SqlError::unsupported(
                "ALTER COLUMN TYPE requires an explicit USING expression".into(),
            ));
        };
        let data_type = super::parse_data_type(clause[..using_offset].trim())?;
        let expression = clause[using_offset + "using".len()..].trim();
        if expression.is_empty() {
            return Err(SqlError::new("USING requires an expression".into()));
        }
        let using = super::super::expr::parse_expression(expression)?;
        return Ok(AlterTableOperation::AlterColumnType {
            field,
            data_type,
            using,
        });
    }

    if lower.starts_with("set default") {
        let default = action["set default".len()..].trim();
        if default.is_empty() {
            return Err(SqlError::new("SET DEFAULT requires a value".into()));
        }
        let mut constraint = FieldConstraint::new(field.clone());
        apply_default_constraint(&mut constraint, default)?;
        return Ok(AlterTableOperation::AlterColumnSetDefault {
            field,
            default_value: constraint.default_value,
            default_expression: constraint.default_expression,
            default_sequence: constraint.default_sequence,
        });
    }
    if lower == "drop default" {
        return Ok(AlterTableOperation::AlterColumnDropDefault { field });
    }
    if let Some(action) = parse_identity_alteration(action, &lower)? {
        return Ok(AlterTableOperation::AlterColumnIdentity { field, action });
    }
    if lower == "set not null" {
        return Ok(AlterTableOperation::AlterColumnSetNotNull { field });
    }
    if lower == "drop not null" {
        return Ok(AlterTableOperation::AlterColumnDropNotNull { field });
    }

    Err(SqlError::new("unsupported ALTER COLUMN operation".into()))
}

pub(super) fn parse_create_sequence_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let after_create = trimmed["create".len()..].trim_start();
    let (persistence, after_kind) = sequence_persistence_prefix(after_create);
    let rest = after_kind
        .get("sequence".len()..)
        .ok_or_else(|| SqlError::new("CREATE SEQUENCE requires a name".into()))?
        .trim();
    let (if_not_exists, rest) = parse_if_not_exists(rest);
    let (name, trailing) = split_first_token(rest)
        .ok_or_else(|| SqlError::new("CREATE SEQUENCE requires a name".into()))?;
    let name = parse_identifier(&name)?;
    if name.is_empty() {
        return Err(SqlError::new("CREATE SEQUENCE requires a name".into()));
    }
    let clauses = parse_sequence_clauses(
        trailing,
        SequenceClauseRules {
            allow_restart: false,
            allow_owned_by: true,
            allow_persistence: false,
            statement: "CREATE SEQUENCE",
        },
    )?;

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CreateSequence(CreateSequenceStatement {
            name,
            if_not_exists,
            data_type: DataType::BigInt,
            options: clauses.options,
            owned_by: clauses.owned_by,
            persistence,
        }),
    })
}

/// `TEMP`/`TEMPORARY`/`UNLOGGED` between `CREATE` and `SEQUENCE`.
fn sequence_persistence_prefix(rest: &str) -> (SequencePersistence, &str) {
    let lower = rest.to_ascii_lowercase();
    for (prefix, persistence) in [
        ("temporary ", SequencePersistence::Temporary),
        ("temp ", SequencePersistence::Temporary),
        ("unlogged ", SequencePersistence::Unlogged),
    ] {
        if lower.starts_with(prefix) {
            return (persistence, rest[prefix.len()..].trim_start());
        }
    }
    (SequencePersistence::Permanent, rest)
}

pub(super) fn parse_alter_sequence_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed["alter sequence".len()..].trim();
    let (if_exists, rest) = parse_if_exists(rest);
    let (name, trailing) = split_first_token(rest)
        .ok_or_else(|| SqlError::new("ALTER SEQUENCE requires a name".into()))?;
    let name = parse_identifier(&name)?;
    let clauses = parse_sequence_clauses(
        trailing,
        SequenceClauseRules {
            allow_restart: true,
            allow_owned_by: true,
            allow_persistence: true,
            statement: "ALTER SEQUENCE",
        },
    )?;
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::AlterSequence(AlterSequenceStatement {
            name,
            if_exists,
            options: clauses.options,
            owned_by: clauses.owned_by,
            persistence: clauses.persistence,
        }),
    })
}

pub(super) fn parse_drop_sequence_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed["drop sequence".len()..].trim();
    let (if_exists, rest) = parse_if_exists(rest);
    let mut names = super::split_csv(rest)
        .iter()
        .map(|name| parse_identifier(name.trim()))
        .collect::<Result<Vec<_>, _>>()?;
    if names.is_empty() || names.iter().any(String::is_empty) {
        return Err(SqlError::new("DROP SEQUENCE requires a name".into()));
    }
    if names
        .iter()
        .any(|name| name.split_whitespace().count() != 1)
    {
        return Err(SqlError::new(
            "DROP SEQUENCE CASCADE and RESTRICT are not supported".into(),
        ));
    }
    let name = names.remove(0);

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::DropSequence(DropSequenceStatement {
            name,
            if_exists,
            additional_names: names,
        }),
    })
}

/// `ADD GENERATED ...`, `SET GENERATED ...`/`SET option`/`RESTART`, or
/// `DROP IDENTITY [IF EXISTS]`.
fn parse_identity_alteration(
    action: &str,
    lower: &str,
) -> Result<Option<crate::sql::ast::IdentityAlteration>, SqlError> {
    use crate::sql::ast::IdentityAlteration;
    if lower == "drop identity" || lower == "drop identity if exists" {
        return Ok(Some(IdentityAlteration::Drop {
            if_exists: lower.ends_with("if exists"),
        }));
    }
    if lower.starts_with("add generated") {
        let mut tokens = super::tokenize_schema_field(&action["add".len()..]).into_iter();
        tokens.next();
        let (generation, _) = super::schema_identity::parse_identity_clause(&mut tokens)?;
        let options = match tokens.next() {
            Some(token) => super::schema_identity::parse_identity_options(&token)?,
            None => crate::catalog::SequenceOptions::default(),
        };
        return Ok(Some(IdentityAlteration::Add {
            generation,
            options,
        }));
    }
    let identity_option = [
        "set generated",
        "restart",
        "set increment",
        "set start",
        "set minvalue",
        "set maxvalue",
        "set no minvalue",
        "set no maxvalue",
        "set no cycle",
        "set cache",
        "set cycle",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix));
    if !identity_option {
        return Ok(None);
    }
    let mut generation = None;
    let mut remaining = Vec::new();
    let mut tokens = action.split_whitespace().peekable();
    while let Some(token) = tokens.next() {
        if token.eq_ignore_ascii_case("set") {
            if tokens
                .peek()
                .is_some_and(|next| next.eq_ignore_ascii_case("generated"))
            {
                tokens.next();
                let mut rest = tokens.by_ref().map(str::to_string).peekable();
                generation = Some(super::schema_identity::parse_generation(&mut rest)?);
                remaining.extend(rest);
                break;
            }
            continue;
        }
        remaining.push(token.to_string());
    }
    let remaining = remaining
        .into_iter()
        .filter(|token| !token.eq_ignore_ascii_case("set"))
        .collect::<Vec<_>>()
        .join(" ");
    let options = super::sequence_options::parse_sequence_clauses(
        &remaining,
        super::sequence_options::SequenceClauseRules {
            allow_restart: true,
            allow_owned_by: false,
            allow_persistence: false,
            statement: "ALTER COLUMN",
        },
    )?
    .options;
    Ok(Some(IdentityAlteration::Set {
        generation,
        options,
    }))
}
