use super::schema_identifiers::{
    parse_column_identifier, parse_column_identifier_list, parse_identifier,
};
use super::schema_indexes::{parse_index_include_fields, parse_index_options};
use super::schema_references::parse_references_target_with_rest;
use super::{
    parse_check_constraint, starts_with_keyword, AlterTableOperation, FieldConstraint,
    FieldDefinition, SqlError,
};

pub(super) fn parse_table_constraint(
    raw: &str,
    table: &str,
) -> Result<Option<Vec<FieldConstraint>>, SqlError> {
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

    let constraints = parse_table_constraint_body(clause, constraint_name.as_deref())?;
    Ok(Some(name_unnamed_composite_primary_key(
        name_unnamed_composite_unique(constraints, table),
        table,
    )))
}

fn name_unnamed_composite_primary_key(
    mut constraints: Vec<FieldConstraint>,
    table: &str,
) -> Vec<FieldConstraint> {
    let primary_key_members = constraints
        .iter()
        .filter(|constraint| constraint.primary_key)
        .count();
    if primary_key_members > 1
        && constraints
            .iter()
            .filter(|constraint| constraint.primary_key)
            .all(|constraint| constraint.primary_key_name.is_none())
    {
        let name = format!("{}_pkey", crate::catalog::local_name(table));
        for constraint in &mut constraints {
            if constraint.primary_key {
                constraint.primary_key_name = Some(name.clone());
            }
        }
    }
    constraints
}

/// Names an unnamed multi-column `UNIQUE (a, b)` as PostgreSQL does,
/// `<table>_<a>_<b>_key`, so its columns share one constraint name.
fn name_unnamed_composite_unique(
    mut constraints: Vec<FieldConstraint>,
    table: &str,
) -> Vec<FieldConstraint> {
    let unnamed_unique = constraints
        .iter()
        .filter(|constraint| constraint.unique && constraint.unique_name.is_none())
        .count();
    if unnamed_unique < 2 {
        return constraints;
    }
    let local_table = crate::catalog::local_name(table);
    let columns = constraints
        .iter()
        .map(|constraint| constraint.field.as_str())
        .collect::<Vec<_>>()
        .join("_");
    let name = format!("{local_table}_{columns}_key");
    for constraint in &mut constraints {
        constraint.unique_name = Some(name.clone());
    }
    constraints
}

pub(super) fn apply_table_constraints(
    fields: &mut [FieldDefinition],
    mut constraints: Vec<FieldConstraint>,
    table: &str,
) -> Result<(), SqlError> {
    for constraint in &mut constraints {
        normalize_constraint_include_fields(fields, constraint)?;
    }
    let existing = fields
        .iter()
        .flat_map(|field| field.constraints.iter().cloned())
        .collect::<Vec<_>>();
    let mut check_fields = std::collections::HashSet::new();
    if existing.iter().chain(&constraints).any(|constraint| {
        constraint.check.is_some()
            && !check_fields.insert(crate::sql::ColumnIdentifierPath::stored_field_key(
                &constraint.field,
            ))
    }) {
        return Err(SqlError::unsupported(
            "multiple CHECK constraints on one field are not supported".into(),
        ));
    }
    if let Some(message) =
        crate::catalog::conflicting_unique_membership(table, &existing, &constraints)
    {
        return Err(SqlError::unsupported(message));
    }
    for mut constraint in constraints {
        let Some(field) = fields.iter_mut().find(|field| {
            crate::sql::ColumnIdentifierPath::stored_field_key(&field.name) == constraint.field
        }) else {
            return Err(SqlError::new(format!(
                "table constraint references unknown column '{}'",
                constraint.field
            )));
        };

        constraint.use_declared_field_spelling(&field.name);
        merge_field_constraint(field, constraint);
    }

    Ok(())
}

pub(super) fn parse_add_constraint(raw: &str) -> Result<AlterTableOperation, SqlError> {
    let raw = raw.trim();
    if starts_with_keyword(raw, "add constraint") {
        let rest = raw["add constraint".len()..].trim_start();
        let (name, body) = split_constraint_name(rest)?;
        if starts_with_keyword(body, "unique using index") {
            return parse_unique_using_index(body, Some(name));
        }
        return Ok(AlterTableOperation::AddConstraint {
            constraints: parse_table_constraint_body(body, Some(name.as_str()))?,
        });
    }

    let body = raw
        .get(..3)
        .filter(|prefix| prefix.eq_ignore_ascii_case("add"))
        .map(|_| raw[3..].trim_start())
        .unwrap_or_default();
    if starts_with_keyword(body, "primary key") {
        return Ok(AlterTableOperation::AddConstraint {
            constraints: parse_table_constraint_body(body, None)?,
        });
    }
    if starts_with_keyword(body, "unique using index") {
        return parse_unique_using_index(body, None);
    }
    Err(SqlError::new("ADD requires a constraint clause".into()))
}

fn parse_unique_using_index(
    raw: &str,
    constraint_name: Option<String>,
) -> Result<AlterTableOperation, SqlError> {
    if !starts_with_keyword(raw, "unique using index") {
        return Err(SqlError::new(
            "UNIQUE USING INDEX requires an index name".into(),
        ));
    }
    let rest = raw["unique using index".len()..].trim_start();
    let (index_name, trailing) = split_constraint_name(rest)?;
    if !trailing.is_empty() {
        return Err(SqlError::unsupported(format!(
            "unsupported UNIQUE USING INDEX option '{trailing}'"
        )));
    }
    Ok(AlterTableOperation::AddUniqueConstraintUsingIndex {
        constraint_name,
        index_name,
    })
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
        let (fields, rest) =
            parse_parenthesized_field_list_with_rest(raw["primary key".len()..].trim_start())?;
        if !rest.trim().is_empty() {
            return Err(SqlError::new("unsupported tokens after field list".into()));
        }
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
        let (nulls_not_distinct, rest) =
            parse_unique_nulls_policy(raw["unique".len()..].trim_start())?;
        let (fields, rest) = parse_parenthesized_field_list_with_rest(rest)?;
        let (include_fields, options) = parse_constraint_index_parameters(rest)?;
        validate_constraint_include_fields(&fields, &include_fields)?;
        let mut constraints = fields
            .into_iter()
            .enumerate()
            .map(|(idx, field)| {
                let mut constraint = FieldConstraint::new(field);
                constraint.unique = true;
                constraint.unique_null_policy = if nulls_not_distinct {
                    crate::catalog::UniqueNullPolicy::NotDistinct
                } else {
                    crate::catalog::UniqueNullPolicy::Distinct
                };
                constraint.unique_name = constraint_name.map(str::to_string);
                constraint.unique_ordinal = Some(u32::try_from(idx + 1).unwrap_or(u32::MAX));
                constraint
            })
            .collect::<Vec<_>>();
        set_constraint_index_parameters(&mut constraints, include_fields, options);
        return Ok(constraints);
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

fn parse_unique_nulls_policy(raw: &str) -> Result<(bool, &str), SqlError> {
    if !starts_with_keyword(raw, "nulls") {
        return Ok((false, raw));
    }
    let rest = raw["nulls".len()..].trim_start();
    if starts_with_keyword(rest, "not") {
        let distinct = rest["not".len()..].trim_start();
        if !starts_with_keyword(distinct, "distinct") {
            return Err(SqlError::new("NULLS NOT requires DISTINCT".into()));
        }
        return Ok((true, distinct["distinct".len()..].trim_start()));
    }
    if starts_with_keyword(rest, "distinct") {
        return Ok((false, rest["distinct".len()..].trim_start()));
    }
    Err(SqlError::new(
        "NULLS requires DISTINCT or NOT DISTINCT".into(),
    ))
}

fn parse_constraint_index_parameters(
    raw: &str,
) -> Result<(Vec<String>, std::collections::BTreeMap<String, String>), SqlError> {
    let (include_fields, remainder) = parse_index_include_fields(raw)?;
    let include_fields = include_fields
        .into_iter()
        .map(|field| parse_column_identifier(&field))
        .collect::<Result<Vec<_>, _>>()?;
    let (mut options, trailing) = parse_index_options(remainder)?;
    if !trailing.trim().is_empty() {
        return Err(SqlError::unsupported(format!(
            "unsupported constraint index option '{}'",
            trailing.trim()
        )));
    }
    for (name, value) in &mut options {
        match name.as_str() {
            "fillfactor" => {
                let fillfactor = value
                    .parse::<u8>()
                    .ok()
                    .filter(|value| (10..=100).contains(value))
                    .ok_or_else(|| {
                        SqlError::new("btree fillfactor must be between 10 and 100".into())
                    })?;
                *value = fillfactor.to_string();
            }
            "deduplicate_items" => {
                *value = match value.to_ascii_lowercase().as_str() {
                    "true" | "on" | "1" => "true".to_string(),
                    "false" | "off" | "0" => "false".to_string(),
                    _ => {
                        return Err(SqlError::new(
                            "btree deduplicate_items must be a boolean".into(),
                        ));
                    }
                };
            }
            _ => {
                return Err(SqlError::unsupported(format!(
                    "unsupported btree storage parameter '{name}'"
                )));
            }
        }
    }
    Ok((include_fields, options))
}

fn set_constraint_index_parameters(
    constraints: &mut [FieldConstraint],
    include_fields: Vec<String>,
    options: std::collections::BTreeMap<String, String>,
) {
    if let Some(first) = constraints.first_mut() {
        first.constraint_index_include_fields = include_fields;
        first.constraint_index_options = options;
    }
}

fn validate_constraint_include_fields(
    key_fields: &[String],
    include_fields: &[String],
) -> Result<(), SqlError> {
    let key_fields = key_fields
        .iter()
        .map(|field| crate::sql::ColumnIdentifierPath::stored_field_key(field))
        .collect::<std::collections::BTreeSet<_>>();
    let mut seen = std::collections::BTreeSet::new();
    for field in include_fields {
        let normalized = crate::sql::ColumnIdentifierPath::stored_field_key(field);
        if !seen.insert(normalized.clone()) {
            return Err(SqlError::new(format!(
                "INCLUDE field '{field}' is duplicated"
            )));
        }
        if key_fields.contains(&normalized) {
            return Err(SqlError::new(format!(
                "INCLUDE field '{field}' duplicates a constraint key field"
            )));
        }
    }
    Ok(())
}

fn normalize_constraint_include_fields(
    fields: &[FieldDefinition],
    constraint: &mut FieldConstraint,
) -> Result<(), SqlError> {
    for include_field in &mut constraint.constraint_index_include_fields {
        let include_key = crate::sql::ColumnIdentifierPath::stored_field_key(include_field);
        let Some(declared) = fields.iter().find(|field| {
            crate::sql::ColumnIdentifierPath::stored_field_key(&field.name) == include_key
        }) else {
            return Err(SqlError::new(format!(
                "constraint INCLUDE references unknown column '{include_field}'"
            )));
        };
        include_field.clone_from(&declared.name);
    }
    Ok(())
}

fn parse_parenthesized_field_list_with_rest(raw: &str) -> Result<(Vec<String>, &str), SqlError> {
    let raw = raw.trim_start();
    if !raw.starts_with('(') {
        return Err(SqlError::new("constraint requires field list".into()));
    }
    let close = super::find_matching_paren(raw, 0)
        .ok_or_else(|| SqlError::new("constraint field list missing closing ')'".into()))?;
    let fields = parse_column_identifier_list(&raw[1..close])?;
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
