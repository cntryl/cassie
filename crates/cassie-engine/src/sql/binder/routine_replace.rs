//! `CREATE [OR REPLACE] FUNCTION|PROCEDURE` conflict rules, following
//! PostgreSQL 18.6: a plain duplicate is 42723, a redefinition may not change
//! the result type or parameter names (42P13) or turn a function into a
//! procedure (42809), and `OR REPLACE` with `IF NOT EXISTS` is a syntax error.
//!
//! Cassie keys routines by name, so PostgreSQL's overloads are unsupported: a
//! redefinition with another parameter list is rejected (0A000) instead of
//! replacing the existing routine. Routine ownership is not recorded, so the
//! owner check of `CREATE OR REPLACE` is not enforced.
use super::{CassieError, Catalog};
use crate::app::DiagnosticError;
use crate::catalog::FunctionArgMeta;
use crate::sql::ast::FunctionArg;
use crate::types::DataType;

fn diagnostic(code: &'static str, message: String) -> CassieError {
    DiagnosticError::new(code, message).into()
}

/// Rejects the clause combination PostgreSQL does not parse.
pub(super) fn check_clauses(
    kind: &str,
    or_replace: bool,
    if_not_exists: bool,
) -> Result<(), CassieError> {
    if or_replace && if_not_exists {
        return Err(diagnostic(
            "42601",
            format!("syntax error: CREATE OR REPLACE {kind} cannot be combined with IF NOT EXISTS"),
        ));
    }
    Ok(())
}

/// The plain `CREATE` of a routine that already exists.
pub(super) fn duplicate(kind: &str, name: &str) -> CassieError {
    diagnostic("42723", format!("{kind} '{name}' already exists"))
}

fn same_arguments(
    kind: &str,
    name: &str,
    existing: &[FunctionArgMeta],
    requested: &[FunctionArg],
) -> Result<(), CassieError> {
    let types_match = existing.len() == requested.len()
        && existing
            .iter()
            .zip(requested)
            .all(|(old, new)| old.data_type == new.data_type);
    if !types_match {
        return Err(diagnostic(
            "0A000",
            format!(
                "{kind} '{name}' already exists with another parameter list; overloaded \
                 routines are not supported"
            ),
        ));
    }
    if existing
        .iter()
        .zip(requested)
        .any(|(old, new)| !old.name.eq_ignore_ascii_case(&new.name))
    {
        return Err(diagnostic(
            "42P13",
            format!("cannot change name of input parameter of {kind} '{name}'"),
        ));
    }
    Ok(())
}

/// Validates `CREATE OR REPLACE FUNCTION` against what is stored under `name`.
pub(super) fn check_function_replace(
    catalog: &Catalog,
    name: &str,
    args: &[FunctionArg],
    return_type: &DataType,
) -> Result<(), CassieError> {
    if catalog.get_procedure(name).is_some() {
        return Err(diagnostic("42809", format!("'{name}' is not a function")));
    }
    let Some(existing) = catalog.get_function(name) else {
        return Ok(());
    };
    same_arguments("function", name, &existing.args, args)?;
    if existing.return_type != *return_type {
        return Err(diagnostic(
            "42P13",
            format!("cannot change return type of existing function '{name}'"),
        ));
    }
    Ok(())
}

/// Validates `CREATE OR REPLACE PROCEDURE` against what is stored under `name`.
pub(super) fn check_procedure_replace(
    catalog: &Catalog,
    name: &str,
    args: &[FunctionArg],
) -> Result<(), CassieError> {
    if catalog.get_function(name).is_some() {
        return Err(diagnostic("42809", format!("'{name}' is not a procedure")));
    }
    match catalog.get_procedure(name) {
        Some(existing) => same_arguments("procedure", name, &existing.args, args),
        None => Ok(()),
    }
}
