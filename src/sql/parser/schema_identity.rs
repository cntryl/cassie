//! `GENERATED { ALWAYS | BY DEFAULT } AS IDENTITY [ ( sequence options ) ]`.
use super::sequence_options::{parse_sequence_clauses, SequenceClauseRules};
use super::{FieldConstraint, SqlError};
use crate::catalog::{DefaultSequenceOwnership, IdentityGeneration, SequenceOptions};

/// Parses the clause after `GENERATED`.
pub(super) fn parse_identity_clause(
    parts: &mut impl Iterator<Item = String>,
) -> Result<(IdentityGeneration, Option<SequenceOptions>), SqlError> {
    let generation = parse_generation(parts)?;
    let mut expect = |keyword: &str| {
        parts
            .next()
            .filter(|token| token.eq_ignore_ascii_case(keyword))
            .ok_or_else(|| SqlError::new(format!("GENERATED requires {}", keyword.to_uppercase())))
    };
    expect("as")?;
    expect("identity")?;
    Ok((generation, None))
}

/// `ALWAYS` or `BY DEFAULT`.
pub(super) fn parse_generation(
    parts: &mut impl Iterator<Item = String>,
) -> Result<IdentityGeneration, SqlError> {
    let token = parts
        .next()
        .ok_or_else(|| SqlError::new("GENERATED requires ALWAYS or BY DEFAULT".into()))?;
    if token.eq_ignore_ascii_case("always") {
        return Ok(IdentityGeneration::Always);
    }
    if token.eq_ignore_ascii_case("by")
        && parts
            .next()
            .is_some_and(|next| next.eq_ignore_ascii_case("default"))
    {
        return Ok(IdentityGeneration::ByDefault);
    }
    Err(SqlError::new(
        "GENERATED requires ALWAYS or BY DEFAULT".into(),
    ))
}

/// Parses a parenthesized identity option list such as `(START WITH 5)`.
pub(super) fn parse_identity_options(token: &str) -> Result<SequenceOptions, SqlError> {
    let inner = token
        .trim()
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
        .ok_or_else(|| SqlError::new("identity options must be parenthesized".into()))?;
    parse_sequence_clauses(
        inner,
        SequenceClauseRules {
            allow_restart: false,
            allow_owned_by: false,
            allow_persistence: false,
            statement: "identity column",
        },
    )
    .map(|clauses| clauses.options)
}

/// Gives an identity column its owned sequence and generated default.
pub(super) fn attach_identity_sequence(constraint: &mut FieldConstraint, table: &str, field: &str) {
    let sequence = crate::catalog::serial_sequence_name(table, field);
    constraint.not_null = true;
    constraint.not_null_ownership = constraint.not_null_ownership.with_explicit();
    constraint.default_sequence = Some(sequence.clone());
    constraint.default_sequence_owned = DefaultSequenceOwnership::owned();
    constraint.default_expression = Some(crate::catalog::canonical_nextval_expression(&sequence));
}
