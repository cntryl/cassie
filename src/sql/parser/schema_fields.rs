use super::schema_identifiers::parse_identifier;
use super::schema_references::parse_references_target;
use super::{
    parse_check_constraint, parse_constraint_literal, parse_data_type, tokenize_schema_field,
    FieldConstraint, FieldDefinition, SqlError,
};
use crate::catalog::DefaultSequenceOwnership;

pub(super) fn parse_field_definition(raw: &str) -> Result<FieldDefinition, SqlError> {
    parse_field_definition_for_table(raw, None)
}

pub(super) fn parse_field_definition_for_table(
    raw: &str,
    table: Option<&str>,
) -> Result<FieldDefinition, SqlError> {
    let mut parts = tokenize_schema_field(raw).into_iter();
    let name = parse_field_name(&mut parts)?;
    let type_token = parse_field_type_token(&mut parts, &name)?;
    let (data_type, serial_sequence) = parse_field_data_type(&type_token, table, &name)?;

    let mut constraint = FieldConstraint::new(name.clone());
    let mut saw_constraint = false;
    if let Some(sequence) = serial_sequence {
        saw_constraint = true;
        constraint.not_null = true;
        constraint.default_sequence = Some(sequence.clone());
        constraint.default_sequence_owned = DefaultSequenceOwnership::owned();
        constraint.default_expression =
            Some(crate::catalog::canonical_nextval_expression(&sequence));
    }
    let mut pending_constraint_name: Option<String> = None;
    while let Some(token) = parts.next() {
        if apply_field_constraint(
            &token,
            &mut parts,
            &mut constraint,
            &mut pending_constraint_name,
            &mut saw_constraint,
        )? {
            break;
        }
    }

    if !saw_constraint {
        return Ok(FieldDefinition {
            name: name.clone(),
            data_type,
            constraints: Vec::new(),
        });
    }

    Ok(FieldDefinition {
        name: name.clone(),
        data_type,
        constraints: vec![constraint],
    })
}

fn parse_field_name(parts: &mut impl Iterator<Item = String>) -> Result<String, SqlError> {
    let name = parts
        .next()
        .ok_or_else(|| SqlError("invalid column definition".into()))?;
    let name = parse_identifier(name.trim())?;
    if name.is_empty() {
        return Err(SqlError("invalid column definition".into()));
    }
    Ok(name)
}

fn parse_field_type_token(
    parts: &mut impl Iterator<Item = String>,
    name: &str,
) -> Result<String, SqlError> {
    let type_token = parts
        .next()
        .ok_or_else(|| SqlError(format!("missing data type for column '{name}'")))?
        .trim()
        .to_string();
    if type_token.is_empty() {
        return Err(SqlError(format!("missing data type for column '{name}'")));
    }
    Ok(type_token)
}

fn apply_field_constraint(
    token: &str,
    parts: &mut impl Iterator<Item = String>,
    constraint: &mut FieldConstraint,
    pending_constraint_name: &mut Option<String>,
    saw_constraint: &mut bool,
) -> Result<bool, SqlError> {
    match token.to_lowercase().as_str() {
        "constraint" => {
            let name = parts
                .next()
                .ok_or_else(|| SqlError("CONSTRAINT requires a name".into()))?;
            *pending_constraint_name = Some(parse_identifier(&name)?);
        }
        "not" => {
            let next = parts
                .next()
                .ok_or_else(|| SqlError("NOT must be followed by NULL".into()))?;
            if !next.eq_ignore_ascii_case("null") {
                return Err(SqlError(format!("unsupported constraint '{token} {next}'")));
            }
            *saw_constraint = true;
            constraint.not_null = true;
        }
        "null" => return Err(SqlError("unexpected NULL constraint".to_string())),
        "unique" => {
            *saw_constraint = true;
            constraint.unique = true;
            constraint.unique_name = pending_constraint_name.take();
            constraint.unique_ordinal = Some(1);
        }
        "primary" => {
            let next = parts
                .next()
                .ok_or_else(|| SqlError("PRIMARY must be followed by KEY".into()))?;
            if !next.eq_ignore_ascii_case("key") {
                return Err(SqlError(format!("unsupported constraint '{token} {next}'")));
            }
            *saw_constraint = true;
            constraint.primary_key = true;
            constraint.primary_key_name = pending_constraint_name.take();
            constraint.primary_key_ordinal = Some(1);
        }
        "key" => return Err(SqlError("KEY without PRIMARY".to_string())),
        "default" => {
            *saw_constraint = true;
            let value = parts
                .next()
                .ok_or_else(|| SqlError("DEFAULT requires a value".into()))?;
            apply_default_constraint(constraint, &value)?;
        }
        "check" => {
            *saw_constraint = true;
            let expression = parts
                .next()
                .ok_or_else(|| SqlError("CHECK requires an expression".into()))?;
            let remaining = parts.collect::<Vec<_>>().join(" ");
            let expression = if remaining.is_empty() {
                expression
            } else {
                format!("{expression} {remaining}")
            };
            let constraint_check = parse_check_constraint(&expression)?;
            constraint.check = Some(constraint_check);
            constraint.check_name = pending_constraint_name.take();
            return Ok(true);
        }
        "references" => {
            *saw_constraint = true;
            let reference = parts
                .next()
                .ok_or_else(|| SqlError("REFERENCES requires target table and column".into()))?;
            let (table, field) = parse_references_target(&reference)?;
            constraint.references_table = Some(table);
            constraint.references_field = Some(field);
            constraint.foreign_key_name = pending_constraint_name.take();
            constraint.foreign_key_ordinal = Some(1);
            constraint.foreign_key_on_delete = Some("NO ACTION".to_string());
            constraint.foreign_key_on_update = Some("NO ACTION".to_string());
        }
        other => return Err(SqlError(format!("unsupported constraint '{other}'"))),
    }
    Ok(false)
}

pub(super) fn apply_default_constraint(
    constraint: &mut FieldConstraint,
    raw: &str,
) -> Result<(), SqlError> {
    if let Some(sequence) = crate::catalog::parse_nextval_default_expression(raw) {
        constraint.default_value = None;
        constraint.default_sequence = Some(sequence.clone());
        constraint.default_sequence_owned = DefaultSequenceOwnership::shared();
        constraint.default_expression =
            Some(crate::catalog::canonical_nextval_expression(&sequence));
        return Ok(());
    }

    constraint.default_value = Some(parse_constraint_literal(raw)?);
    constraint.default_expression = None;
    constraint.default_sequence = None;
    Ok(())
}

fn parse_field_data_type(
    raw: &str,
    table: Option<&str>,
    field: &str,
) -> Result<(crate::types::DataType, Option<String>), SqlError> {
    match raw.to_ascii_lowercase().as_str() {
        "serial" | "serial4" => {
            let table = table.ok_or_else(|| SqlError("SERIAL requires a table name".into()))?;
            Ok((
                crate::types::DataType::Int,
                Some(crate::catalog::serial_sequence_name(table, field)),
            ))
        }
        "bigserial" | "serial8" => {
            let table = table.ok_or_else(|| SqlError("BIGSERIAL requires a table name".into()))?;
            Ok((
                crate::types::DataType::BigInt,
                Some(crate::catalog::serial_sequence_name(table, field)),
            ))
        }
        _ => parse_data_type(raw).map(|data_type| (data_type, None)),
    }
}
