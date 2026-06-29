use super::schema_fields::apply_default_constraint;
use super::schema_identifiers::parse_identifier;
use super::{AlterTableOperation, SqlError, split_first_token, FieldConstraint, ParsedStatement, parse_if_not_exists, QueryStatement, CreateSequenceStatement, DataType, parse_if_exists, DropSequenceStatement};

pub(super) fn parse_alter_column_operation(raw: &str) -> Result<AlterTableOperation, SqlError> {
    let (field, action) = split_first_token(raw)
        .ok_or_else(|| SqlError("ALTER COLUMN requires a column name".into()))?;
    let field = parse_identifier(&field)?;
    let action = action.trim();
    let lower = action.to_ascii_lowercase();

    if lower.starts_with("set default") {
        let default = action["set default".len()..].trim();
        if default.is_empty() {
            return Err(SqlError("SET DEFAULT requires a value".into()));
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
    if lower == "set not null" {
        return Ok(AlterTableOperation::AlterColumnSetNotNull { field });
    }
    if lower == "drop not null" {
        return Ok(AlterTableOperation::AlterColumnDropNotNull { field });
    }

    Err(SqlError("unsupported ALTER COLUMN operation".into()))
}

pub(super) fn parse_create_sequence_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed["create sequence".len()..].trim();
    let (if_not_exists, rest) = parse_if_not_exists(rest)?;
    let (name, trailing) = split_first_token(rest)
        .ok_or_else(|| SqlError("CREATE SEQUENCE requires a name".into()))?;
    let name = parse_identifier(&name)?;
    if name.is_empty() {
        return Err(SqlError("CREATE SEQUENCE requires a name".into()));
    }
    let trailing = trailing.trim();
    if !trailing.is_empty() {
        return Err(SqlError(format!(
            "unsupported CREATE SEQUENCE option '{}'",
            trailing
                .split_whitespace()
                .next()
                .unwrap_or(trailing)
                .to_ascii_uppercase()
        )));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CreateSequence(CreateSequenceStatement {
            name,
            if_not_exists,
            data_type: DataType::Int,
        }),
    })
}

pub(super) fn parse_drop_sequence_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed["drop sequence".len()..].trim();
    let (if_exists, rest) = parse_if_exists(rest)?;
    let name = parse_identifier(rest)?;
    if name.is_empty() {
        return Err(SqlError("DROP SEQUENCE requires a name".into()));
    }
    if name.split_whitespace().count() != 1 {
        return Err(SqlError(
            "DROP SEQUENCE supports only a single sequence name".into(),
        ));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::DropSequence(DropSequenceStatement { name, if_exists }),
    })
}
