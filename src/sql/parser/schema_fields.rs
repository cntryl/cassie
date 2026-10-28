use super::schema_identifiers::parse_column_identifier;
use super::schema_references::parse_references_target;
use super::{
    parse_check_constraint, parse_constant_literal, parse_data_type, tokenize_schema_field,
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
    if super::super::clauses::find_top_level_keyword_span(raw, 0, "collate").is_some() {
        return Err(SqlError::unsupported(
            "column collation metadata is not supported in this version".into(),
        ));
    }
    let mut parts = tokenize_schema_field(raw).into_iter().peekable();
    let name = parse_field_name(&mut parts)?;
    let type_token = parse_field_type_token(&mut parts, &name)?;
    let (data_type, serial_sequence) = parse_field_data_type(&type_token, table, &name)?;

    let mut constraint = FieldConstraint::new(name.clone());
    let mut saw_constraint = false;
    if let Some(sequence) = serial_sequence {
        saw_constraint = true;
        constraint.not_null = true;
        constraint.not_null_ownership = constraint.not_null_ownership.with_explicit();
        constraint.default_sequence = Some(sequence.clone());
        constraint.default_sequence_owned = DefaultSequenceOwnership::owned();
        constraint.default_expression =
            Some(crate::catalog::canonical_nextval_expression(&sequence));
    }
    let remaining_parts = parts.collect::<Vec<_>>();
    if remaining_parts
        .iter()
        .filter(|token| token.eq_ignore_ascii_case("check"))
        .count()
        > 1
    {
        return Err(SqlError::unsupported(
            "multiple CHECK constraints on one field are not supported".into(),
        ));
    }
    let mut parts = remaining_parts.into_iter();
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

    if constraint.identity.is_some() {
        let table = table.ok_or_else(|| SqlError::new("IDENTITY requires a table name".into()))?;
        super::schema_identity::attach_identity_sequence(&mut constraint, table, &name);
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
        .ok_or_else(|| SqlError::new("invalid column definition".into()))?;
    let name = parse_column_identifier(name.trim())?;
    if name.is_empty() {
        return Err(SqlError::new("invalid column definition".into()));
    }
    Ok(name)
}

fn parse_field_type_token(
    parts: &mut std::iter::Peekable<impl Iterator<Item = String>>,
    name: &str,
) -> Result<String, SqlError> {
    let mut type_token = parts
        .next()
        .ok_or_else(|| SqlError::new(format!("missing data type for column '{name}'")))?
        .trim()
        .to_string();
    // Bit/character VARYING modifiers and DOUBLE PRECISION span schema tokens.
    if type_token.eq_ignore_ascii_case("bit")
        && parts.peek().is_some_and(|next| is_varying_type_token(next))
    {
        if let Some(varying) = parts.next() {
            type_token = format!("{type_token} {varying}");
        }
    }
    if type_token.eq_ignore_ascii_case("double")
        && parts
            .peek()
            .is_some_and(|next| next.trim().eq_ignore_ascii_case("precision"))
    {
        if let Some(precision) = parts.next() {
            type_token = format!("{type_token} {precision}");
        }
    }
    if type_token.eq_ignore_ascii_case("national")
        && parts
            .peek()
            .is_some_and(|next| next.eq_ignore_ascii_case("character"))
    {
        let character = parts
            .next()
            .ok_or_else(|| SqlError::new("NATIONAL CHARACTER requires VARYING".into()))?;
        let varying = parts
            .next_if(|next| is_varying_type_token(next))
            .ok_or_else(|| SqlError::new("NATIONAL CHARACTER requires VARYING".into()))?;
        type_token = format!("{type_token} {character} {varying}");
    } else if (type_token.eq_ignore_ascii_case("character")
        || type_token.eq_ignore_ascii_case("char")
        || type_token.eq_ignore_ascii_case("nchar"))
        && parts.peek().is_some_and(|next| is_varying_type_token(next))
    {
        if let Some(varying) = parts.next() {
            type_token = format!("{type_token} {varying}");
        }
    }
    let temporal_name = type_token
        .split_once('(')
        .map_or(type_token.as_str(), |(name, _)| name)
        .trim();
    if temporal_name.eq_ignore_ascii_case("time") || temporal_name.eq_ignore_ascii_case("timestamp")
    {
        if let Some(zone_modifier) = parts.next_if(|next| {
            next.eq_ignore_ascii_case("with") || next.eq_ignore_ascii_case("without")
        }) {
            let time = parts.next().ok_or_else(|| {
                SqlError::new(format!("{zone_modifier} must be followed by TIME ZONE"))
            })?;
            let zone = parts.next().ok_or_else(|| {
                SqlError::new(format!("{zone_modifier} TIME must be followed by ZONE"))
            })?;
            if !time.eq_ignore_ascii_case("time") || !zone.eq_ignore_ascii_case("zone") {
                return Err(SqlError::new(format!(
                    "unsupported temporal type qualifier '{zone_modifier} {time} {zone}'"
                )));
            }
            type_token = format!("{type_token} {zone_modifier} {time} {zone}");
        }
    }
    if parts
        .peek()
        .is_some_and(|next| is_sql_standard_array_suffix_token(next))
    {
        let suffix = parts
            .next()
            .ok_or_else(|| SqlError::new("missing SQL array type suffix".into()))?;
        let is_array_keyword = suffix.eq_ignore_ascii_case("array");
        type_token = format!("{type_token} {suffix}");
        if is_array_keyword
            && parts
                .peek()
                .is_some_and(|next| next.trim_start().starts_with('['))
        {
            let dimensions = parts
                .next()
                .ok_or_else(|| SqlError::new("missing SQL array dimensions".into()))?;
            type_token.push_str(&dimensions);
        }
    }
    if type_token.is_empty() {
        return Err(SqlError::new(format!(
            "missing data type for column '{name}'"
        )));
    }
    Ok(type_token)
}

fn is_sql_standard_array_suffix_token(token: &str) -> bool {
    let token = token.trim();
    token.eq_ignore_ascii_case("array")
        || token
            .get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("array"))
            && token[5..].trim_start().starts_with('[')
}

fn is_varying_type_token(token: &str) -> bool {
    token.eq_ignore_ascii_case("varying")
        || token
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("varying("))
}

fn apply_field_constraint(
    token: &str,
    parts: &mut impl Iterator<Item = String>,
    constraint: &mut FieldConstraint,
    pending_constraint_name: &mut Option<String>,
    saw_constraint: &mut bool,
) -> Result<bool, SqlError> {
    if is_column_storage_option(token) {
        return Err(SqlError::unsupported(format!(
            "column {token} options are not supported"
        )));
    }
    match token.to_lowercase().as_str() {
        "constraint" => {
            let name = parts
                .next()
                .ok_or_else(|| SqlError::new("CONSTRAINT requires a name".into()))?;
            *pending_constraint_name = Some(super::schema_identifiers::parse_identifier(&name)?);
        }
        "not" => apply_not_null_constraint(token, parts, constraint, saw_constraint)?,
        "null" => return Err(SqlError::new("unexpected NULL constraint".to_string())),
        "unique" => {
            *saw_constraint = true;
            constraint.unique = true;
            constraint.unique_name = pending_constraint_name.take();
            constraint.unique_ordinal = Some(1);
        }
        "nulls" => {
            constraint.unique_null_policy =
                parse_field_unique_null_policy(parts, constraint.unique)?;
        }
        "primary" => {
            let next = parts
                .next()
                .ok_or_else(|| SqlError::new("PRIMARY must be followed by KEY".into()))?;
            if !next.eq_ignore_ascii_case("key") {
                return Err(SqlError::new(format!(
                    "unsupported constraint '{token} {next}'"
                )));
            }
            *saw_constraint = true;
            constraint.not_null = true;
            constraint.not_null_ownership = constraint.not_null_ownership.with_primary_key();
            constraint.primary_key = true;
            constraint.primary_key_name = pending_constraint_name.take();
            constraint.primary_key_ordinal = Some(1);
        }
        "key" => return Err(SqlError::new("KEY without PRIMARY".to_string())),
        "default" => {
            *saw_constraint = true;
            let value = parts
                .next()
                .ok_or_else(|| SqlError::new("DEFAULT requires a value".into()))?;
            apply_default_constraint(constraint, &value)?;
        }
        "check" => {
            *saw_constraint = true;
            let expression = parts
                .next()
                .ok_or_else(|| SqlError::new("CHECK requires an expression".into()))?;
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
            let reference = parts.next().ok_or_else(|| {
                SqlError::new("REFERENCES requires target table and column".into())
            })?;
            let (table, field) = parse_references_target(&reference)?;
            constraint.references_table = Some(table);
            constraint.references_field = Some(field);
            constraint.foreign_key_name = pending_constraint_name.take();
            constraint.foreign_key_ordinal = Some(1);
            constraint.foreign_key_on_delete = Some("NO ACTION".to_string());
            constraint.foreign_key_on_update = Some("NO ACTION".to_string());
        }
        "generated" => {
            *saw_constraint = true;
            let (generation, _) = super::schema_identity::parse_identity_clause(parts)?;
            constraint.identity = Some(generation);
        }
        token if token.starts_with('(') && constraint.identity.is_some() => {
            constraint.identity_options =
                Some(super::schema_identity::parse_identity_options(token)?);
        }
        other => return Err(SqlError::new(format!("unsupported constraint '{other}'"))),
    }
    Ok(false)
}

fn apply_not_null_constraint(
    token: &str,
    parts: &mut impl Iterator<Item = String>,
    constraint: &mut FieldConstraint,
    saw_constraint: &mut bool,
) -> Result<(), SqlError> {
    let next = parts
        .next()
        .ok_or_else(|| SqlError::new("NOT must be followed by NULL".into()))?;
    if !next.eq_ignore_ascii_case("null") {
        return Err(SqlError::new(format!(
            "unsupported constraint '{token} {next}'"
        )));
    }
    *saw_constraint = true;
    constraint.not_null = true;
    constraint.not_null_ownership = constraint.not_null_ownership.with_explicit();
    Ok(())
}

fn is_column_storage_option(token: &str) -> bool {
    token.eq_ignore_ascii_case("storage") || token.eq_ignore_ascii_case("compression")
}

fn parse_field_unique_null_policy(
    parts: &mut impl Iterator<Item = String>,
    unique: bool,
) -> Result<crate::catalog::UniqueNullPolicy, SqlError> {
    if !unique {
        return Err(SqlError::unsupported(
            "NULLS [NOT] DISTINCT requires a UNIQUE constraint".to_string(),
        ));
    }
    let next = parts
        .next()
        .ok_or_else(|| SqlError::new("NULLS requires DISTINCT or NOT DISTINCT".into()))?;
    if next.eq_ignore_ascii_case("not") {
        let distinct = parts
            .next()
            .ok_or_else(|| SqlError::new("NULLS NOT requires DISTINCT".into()))?;
        if !distinct.eq_ignore_ascii_case("distinct") {
            return Err(SqlError::new("NULLS NOT requires DISTINCT".into()));
        }
        Ok(crate::catalog::UniqueNullPolicy::NotDistinct)
    } else if next.eq_ignore_ascii_case("distinct") {
        Ok(crate::catalog::UniqueNullPolicy::Distinct)
    } else {
        Err(SqlError::new(
            "NULLS requires DISTINCT or NOT DISTINCT".into(),
        ))
    }
}

pub(super) fn apply_default_constraint(
    constraint: &mut FieldConstraint,
    raw: &str,
) -> Result<(), SqlError> {
    crate::sql::definition_guard::expression_sql(raw)
        .map_err(|error| SqlError::unsupported(error.to_string()))?;

    if let Some(sequence) = crate::catalog::parse_nextval_default_expression(raw) {
        constraint.default_value = None;
        constraint.default_sequence = Some(sequence.clone());
        constraint.default_sequence_owned = DefaultSequenceOwnership::shared();
        constraint.default_expression =
            Some(crate::catalog::canonical_nextval_expression(&sequence));
        return Ok(());
    }

    if let Some(expression) = crate::catalog::parse_volatile_default_expression(raw) {
        constraint.default_value = None;
        constraint.default_expression = Some(expression);
        constraint.default_sequence = None;
        return Ok(());
    }

    constraint.default_value = Some(parse_constant_literal(raw).map_err(|_| {
        SqlError::unsupported(format!(
            "DEFAULT {} is not supported: use a constant, nextval(...), now(), CURRENT_TIMESTAMP, LOCALTIMESTAMP, CURRENT_DATE or gen_random_uuid()",
            raw.trim()
        ))
    })?);
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
        "smallserial" | "serial2" => {
            let table =
                table.ok_or_else(|| SqlError::new("SMALLSERIAL requires a table name".into()))?;
            Ok((
                crate::types::DataType::SmallInt,
                Some(crate::catalog::serial_sequence_name(table, field)),
            ))
        }
        "serial" | "serial4" => {
            let table =
                table.ok_or_else(|| SqlError::new("SERIAL requires a table name".into()))?;
            Ok((
                crate::types::DataType::Int,
                Some(crate::catalog::serial_sequence_name(table, field)),
            ))
        }
        "bigserial" | "serial8" => {
            let table =
                table.ok_or_else(|| SqlError::new("BIGSERIAL requires a table name".into()))?;
            Ok((
                crate::types::DataType::BigInt,
                Some(crate::catalog::serial_sequence_name(table, field)),
            ))
        }
        _ => parse_data_type(raw).map(|data_type| (data_type, None)),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_field_type_token;

    #[test]
    fn should_parse_sql_standard_array_type_suffixes_without_declared_bounds() {
        // Arrange
        let declarations = [
            "value INTEGER ARRAY[4]",
            "value INTEGER ARRAY",
            "value INTEGER ARRAY [4]",
            "value VARCHAR(12) ARRAY[4]",
            "value pg_catalog.int4 ARRAY[4]",
        ];
        let expected = [
            crate::types::DataType::Array(Box::new(crate::types::DataType::Int)),
            crate::types::DataType::Array(Box::new(crate::types::DataType::Int)),
            crate::types::DataType::Array(Box::new(crate::types::DataType::Int)),
            crate::types::DataType::Array(Box::new(crate::types::DataType::Varchar {
                length: Some(12),
            })),
            crate::types::DataType::Array(Box::new(crate::types::DataType::Int)),
        ];

        // Act
        let parsed = declarations.map(|declaration| {
            super::parse_field_definition(declaration)
                .expect("parse SQL-standard array declaration")
                .data_type
        });

        // Assert
        for (data_type, expected) in parsed.into_iter().zip(expected) {
            assert_eq!(data_type, expected);
        }
        assert!(super::parse_data_type("INTEGER ARRAY[not-a-number]").is_err());
        assert!(super::parse_data_type("INTEGER ARRAY[4] trailing").is_err());
        assert!(super::parse_data_type("INTEGER[] ARRAY[4]").is_err());
        assert!(super::parse_data_type("JSONPATH ARRAY[4]").is_err());
        let quoted_type = super::parse_data_type("\"custom array\"")
            .expect_err("quoted type name is not an array suffix");
        assert!(!quoted_type
            .to_string()
            .contains("SQL array type dimensions"));
        let unicode_type =
            super::parse_data_type("\"café array\"").expect_err("unknown quoted Unicode type name");
        assert!(!unicode_type
            .to_string()
            .contains("SQL array type dimensions"));
        let escaped_quoted_type = super::parse_data_type("\"custom\"\" array\"")
            .expect_err("unknown quoted type name with an escaped quote");
        assert!(!escaped_quoted_type
            .to_string()
            .contains("SQL array type dimensions"));
        for not_a_suffix in [
            "INTEGERARRAY[4]",
            "INTEGER ARRAY[4] ARRAY[5]",
            "VARCHAR(12 ARRAY[4])",
            "\"custom array\" ARRAY[4]",
        ] {
            let error = super::parse_data_type(not_a_suffix)
                .expect_err("only one final top-level ARRAY suffix is supported");
            assert!(!error.to_string().contains("SQL array type dimensions"));
        }
    }

    #[test]
    fn should_join_varying_only_at_the_type_name_boundary() {
        // Arrange
        let accepted = [
            ("bit", "varying", "bit varying"),
            ("bit", "VARYING(4)", "bit VARYING(4)"),
            ("character", "varying(4)", "character varying(4)"),
            ("nchar", "VARYING(4)", "nchar VARYING(4)"),
        ];
        let rejected = [
            ("bit", "varying_extra(4)"),
            ("character", "varying_extra(4)"),
        ];

        // Act
        let accepted_results = accepted.map(|(type_name, modifier, expected)| {
            let mut parts = [type_name.to_owned(), modifier.to_owned()]
                .into_iter()
                .peekable();
            let parsed = parse_field_type_token(&mut parts, "value").unwrap();
            (expected, parsed, parts.next())
        });
        let rejected_results = rejected.map(|(type_name, modifier)| {
            let mut parts = [type_name.to_owned(), modifier.to_owned()]
                .into_iter()
                .peekable();
            let parsed = parse_field_type_token(&mut parts, "value").unwrap();
            (type_name, modifier, parsed, parts.next())
        });

        // Assert
        for (expected, parsed, remaining) in accepted_results {
            assert_eq!(parsed, expected);
            assert!(remaining.is_none());
        }
        for (type_name, modifier, parsed, remaining) in rejected_results {
            assert_eq!(parsed, type_name);
            assert_eq!(remaining.as_deref(), Some(modifier));
        }
    }
}
