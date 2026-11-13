//! `regoper` and `regoperator` over the built-in `pg_operator` rows. Routine
//! names resolve in `user_routines`.
use super::names::{builtin_type_oid, qualified_name, signature, Identifier};
use crate::catalog::builtin_identity::builtin_identities;
use crate::types::{ObjectIdKind, ObjectIdentifierError};

/// The operator name with an optional `pg_catalog` qualifier;
/// `None` names an object in a schema that holds no built-ins.
fn builtin_name(text: &str) -> Result<Option<String>, ObjectIdentifierError> {
    let parts = qualified_name(text)?;
    match parts.as_slice() {
        [name] => Ok(Some(name.value.clone())),
        [Identifier { value, .. }, name] if value == "pg_catalog" => Ok(Some(name.value.clone())),
        [_, _] => Ok(None),
        _ => Err(ObjectIdentifierError::InvalidName(text.to_string())),
    }
}

pub(super) fn resolve_operator(text: &str) -> Result<Option<u32>, ObjectIdentifierError> {
    let Some(name) = builtin_name(text)? else {
        return Ok(None);
    };
    let mut candidates = builtin_identities().operators_named(&name);
    match (candidates.next(), candidates.next()) {
        (None, _) => Ok(None),
        (Some(row), None) => Ok(Some(row.oid)),
        (Some(_), Some(_)) => Err(ObjectIdentifierError::Ambiguous {
            kind: ObjectIdKind::RegOper,
            name: text.trim().to_string(),
        }),
    }
}

pub(super) fn resolve_operator_signature(text: &str) -> Result<Option<u32>, ObjectIdentifierError> {
    let (name, arguments) = signature(text)?;
    let [left, right] = arguments.as_slice() else {
        return Err(ObjectIdentifierError::Signature(
            "expected two type names in an operator signature".to_string(),
        ));
    };
    let left = operand_type_oid(left)?;
    let right = operand_type_oid(right)?;
    let Some(name) = builtin_name(&name)? else {
        return Ok(None);
    };
    Ok(builtin_identities()
        .operators_named(&name)
        .find(|row| row.left == left && row.right == right)
        .map(|row| row.oid))
}

fn argument_type_oids(arguments: &[String]) -> Result<Vec<u32>, ObjectIdentifierError> {
    arguments
        .iter()
        .map(|argument| {
            builtin_type_oid(argument)?.ok_or_else(|| ObjectIdentifierError::Undefined {
                kind: ObjectIdKind::RegType,
                name: argument.clone(),
            })
        })
        .collect()
}

fn operand_type_oid(text: &str) -> Result<u32, ObjectIdentifierError> {
    if text.trim().eq_ignore_ascii_case("none") {
        return Ok(0);
    }
    argument_type_oids(std::slice::from_ref(&text.to_string())).map(|oids| oids[0])
}

pub(super) fn operator_label(oid: u32) -> Option<String> {
    let identities = builtin_identities();
    let row = identities.operator_by_oid(oid)?;
    let unique = identities.operators_named(&row.name).nth(1).is_none();
    Some(if unique {
        row.name.clone()
    } else {
        format!("pg_catalog.{}", row.name)
    })
}

pub(super) fn operator_signature_label(oid: u32) -> Option<String> {
    let row = builtin_identities().operator_by_oid(oid)?;
    let operand = |oid: u32| {
        if oid == 0 {
            "NONE".to_string()
        } else {
            type_display(oid)
        }
    };
    Some(format!(
        "{}({},{})",
        row.name,
        operand(row.left),
        operand(row.right)
    ))
}

fn type_display(oid: u32) -> String {
    builtin_identities()
        .type_by_oid(oid)
        .map_or_else(|| oid.to_string(), |row| row.display.clone())
}

#[cfg(test)]
mod tests {
    use super::{operator_signature_label, resolve_operator_signature};

    #[test]
    fn should_resolve_operator_signatures_with_none_operands() {
        // Arrange
        let spelling = "+(integer,integer)";

        // Act
        let resolved = resolve_operator_signature(spelling).ok().flatten();

        // Assert
        assert_eq!(resolved, Some(551));
        assert_eq!(
            operator_signature_label(551).as_deref(),
            Some("+(integer,integer)")
        );
    }
}
