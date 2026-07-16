use super::schema_identifiers::{parse_identifier, parse_identifier_list};
use super::schema_references::parse_references_target_with_rest;
use super::{
    parse_check_constraint, starts_with_keyword, FieldConstraint, FieldDefinition, SqlError,
};

pub(super) fn parse_table_constraint(raw: &str) -> Result<Option<Vec<FieldConstraint>>, SqlError> {
    let mut clause = raw.trim();
    let mut constraint_name = None;
    if !starts_with_keyword(clause, "constraint")
        && !starts_with_keyword(clause, "primary")
        && !starts_with_keyword(clause, "unique")
        && !starts_with_keyword(clause, "foreign")
        && !starts_with_keyword(clause, "check")
    {
        return Ok(None);
    }

    if starts_with_keyword(clause, "constraint") {
        clause = clause["constraint".len()..].trim_start();
        let (name, rest) = split_constraint_name(clause)?;
        constraint_name = Some(name);
        clause = rest;
    }

    parse_table_constraint_body(clause, constraint_name.as_deref()).map(Some)
}

pub(super) fn apply_table_constraints(
    fields: &mut [FieldDefinition],
    constraints: Vec<FieldConstraint>,
) -> Result<(), SqlError> {
    for constraint in constraints {
        let Some(field) = fields
            .iter_mut()
            .find(|field| field.name.eq_ignore_ascii_case(&constraint.field))
        else {
            return Err(SqlError::new(format!(
                "table constraint references unknown column '{}'",
                constraint.field
            )));
        };

        merge_field_constraint(field, constraint);
    }

    Ok(())
}

pub(super) fn parse_named_add_constraint(raw: &str) -> Result<Vec<FieldConstraint>, SqlError> {
    let raw = raw.trim();
    if !starts_with_keyword(raw, "add constraint") {
        return Err(SqlError::new(
            "ADD CONSTRAINT requires a constraint clause".into(),
        ));
    }

    let rest = raw["add constraint".len()..].trim_start();
    let (name, body) = split_constraint_name(rest)?;
    parse_table_constraint_body(body, Some(name.as_str()))
}

fn split_constraint_name(raw: &str) -> Result<(String, &str), SqlError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(SqlError::new("CONSTRAINT requires a name".into()));
    }

    if raw.starts_with('"') {
        let mut escaped = false;
        for (idx, ch) in raw.char_indices().skip(1) {
            if ch != '"' {
                escaped = false;
                continue;
            }
            if escaped {
                escaped = false;
                continue;
            }
            if raw[idx + ch.len_utf8()..].starts_with('"') {
                escaped = true;
                continue;
            }
            let name = parse_identifier(&raw[..=idx])?;
            return Ok((name, raw[idx + ch.len_utf8()..].trim_start()));
        }
        return Err(SqlError::new(format!(
            "unterminated quoted identifier '{raw}'"
        )));
    }

    let mut parts = raw.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or_default();
    Ok((
        parse_identifier(name)?,
        parts.next().unwrap_or_default().trim_start(),
    ))
}

fn parse_table_constraint_body(
    raw: &str,
    constraint_name: Option<&str>,
) -> Result<Vec<FieldConstraint>, SqlError> {
    let raw = raw.trim();
    if starts_with_keyword(raw, "primary key") {
        let fields = parse_parenthesized_field_list(raw["primary key".len()..].trim_start())?;
        return Ok(fields
            .into_iter()
            .enumerate()
            .map(|(idx, field)| {
                let mut constraint = FieldConstraint::new(field);
                constraint.not_null = true;
                constraint.not_null_ownership = constraint.not_null_ownership.with_primary_key();
                constraint.primary_key = true;
                constraint.primary_key_name = constraint_name.map(str::to_string);
                constraint.primary_key_ordinal = Some(u32::try_from(idx + 1).unwrap_or(u32::MAX));
                constraint
            })
            .collect());
    }

    if starts_with_keyword(raw, "unique") {
        let fields = parse_parenthesized_field_list(raw["unique".len()..].trim_start())?;
        return Ok(fields
            .into_iter()
            .enumerate()
            .map(|(idx, field)| {
                let mut constraint = FieldConstraint::new(field);
                constraint.unique = true;
                constraint.unique_name = constraint_name.map(str::to_string);
                constraint.unique_ordinal = Some(u32::try_from(idx + 1).unwrap_or(u32::MAX));
                constraint
            })
            .collect());
    }

    if starts_with_keyword(raw, "foreign key") {
        let rest = raw["foreign key".len()..].trim_start();
        let (fields, rest) = parse_parenthesized_field_list_with_rest(rest)?;
        let references = rest.trim_start();
        if !starts_with_keyword(references, "references") {
            return Err(SqlError::new(
                "FOREIGN KEY requires REFERENCES clause".into(),
            ));
        }
        let reference = references["references".len()..].trim_start();
        let (table, field, rest) = parse_references_target_with_rest(reference)?;
        let (on_delete, on_update) = parse_foreign_key_actions(rest)?;
        if fields.len() != 1 {
            return Err(SqlError::new(
                "FOREIGN KEY supports exactly one source column".into(),
            ));
        }
        let mut constraint = FieldConstraint::new(fields[0].clone());
        constraint.references_table = Some(table);
        constraint.references_field = Some(field);
        constraint.foreign_key_name = constraint_name.map(str::to_string);
        constraint.foreign_key_ordinal = Some(1);
        constraint.foreign_key_on_delete = Some(on_delete);
        constraint.foreign_key_on_update = Some(on_update);
        return Ok(vec![constraint]);
    }

    if starts_with_keyword(raw, "check") {
        let check = parse_check_constraint(raw["check".len()..].trim_start())?;
        let mut constraint = FieldConstraint::new(check.field.clone());
        constraint.check = Some(check);
        constraint.check_name = constraint_name.map(str::to_string);
        return Ok(vec![constraint]);
    }

    Err(SqlError::new("unsupported table constraint".into()))
}

fn parse_parenthesized_field_list(raw: &str) -> Result<Vec<String>, SqlError> {
    let (fields, rest) = parse_parenthesized_field_list_with_rest(raw)?;
    if !rest.trim().is_empty() {
        return Err(SqlError::new("unsupported tokens after field list".into()));
    }
    Ok(fields)
}

fn parse_parenthesized_field_list_with_rest(raw: &str) -> Result<(Vec<String>, &str), SqlError> {
    let raw = raw.trim_start();
    if !raw.starts_with('(') {
        return Err(SqlError::new("constraint requires field list".into()));
    }
    let close = super::find_matching_paren(raw, 0)
        .ok_or_else(|| SqlError::new("constraint field list missing closing ')'".into()))?;
    let fields = parse_identifier_list(&raw[1..close])?;
    if fields.is_empty() || fields.iter().any(|field| field.trim().is_empty()) {
        return Err(SqlError::new(
            "constraint field list cannot be empty".into(),
        ));
    }
    Ok((fields, &raw[close + 1..]))
}

fn merge_field_constraint(field: &mut FieldDefinition, constraint: FieldConstraint) {
    crate::catalog::merge_constraint_set(&mut field.constraints, [constraint]);
}

fn parse_foreign_key_actions(raw: &str) -> Result<(String, String), SqlError> {
    let tokens = super::tokenize_schema_field(raw);
    let mut on_delete = "NO ACTION".to_string();
    let mut on_update = "NO ACTION".to_string();
    let mut index = 0;
    while index < tokens.len() {
        if !tokens[index].eq_ignore_ascii_case("on") {
            return Err(SqlError::new(format!(
                "unsupported FOREIGN KEY option '{}'",
                tokens[index]
            )));
        }
        index += 1;
        let Some(target) = tokens.get(index) else {
            return Err(SqlError::new("ON requires DELETE or UPDATE".into()));
        };
        index += 1;
        let (action, consumed) = parse_foreign_key_action(&tokens[index..])?;
        index += consumed;
        if target.eq_ignore_ascii_case("delete") {
            on_delete = action;
        } else if target.eq_ignore_ascii_case("update") {
            on_update = action;
        } else {
            return Err(SqlError::new("ON requires DELETE or UPDATE".into()));
        }
    }
    Ok((on_delete, on_update))
}

fn parse_foreign_key_action(tokens: &[String]) -> Result<(String, usize), SqlError> {
    let Some(first) = tokens.first() else {
        return Err(SqlError::new("FOREIGN KEY action is missing".into()));
    };
    if first.eq_ignore_ascii_case("cascade") {
        return Ok(("CASCADE".to_string(), 1));
    }
    if first.eq_ignore_ascii_case("restrict") {
        return Ok(("RESTRICT".to_string(), 1));
    }
    if first.eq_ignore_ascii_case("no") {
        let Some(second) = tokens.get(1) else {
            return Err(SqlError::new("NO must be followed by ACTION".into()));
        };
        if second.eq_ignore_ascii_case("action") {
            return Ok(("NO ACTION".to_string(), 2));
        }
    }
    if first.eq_ignore_ascii_case("set") {
        let Some(second) = tokens.get(1) else {
            return Err(SqlError::new(
                "SET must be followed by NULL or DEFAULT".into(),
            ));
        };
        if second.eq_ignore_ascii_case("null") {
            return Ok(("SET NULL".to_string(), 2));
        }
        if second.eq_ignore_ascii_case("default") {
            return Ok(("SET DEFAULT".to_string(), 2));
        }
    }
    Err(SqlError::new(format!(
        "unsupported FOREIGN KEY action '{first}'"
    )))
}
