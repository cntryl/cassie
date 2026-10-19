//! SQL `PREPARE`, `EXECUTE` and `DEALLOCATE` over the session's shared
//! prepared-statement namespace.

use super::{command_result, SessionObjectCall, SessionObjectError};
use crate::app::{
    Cassie, CassieError, CassieSession, QueryExecutionControls, QueryResult, SessionPrepared,
    MAX_PREPARED_PER_SESSION,
};
use crate::sql::ast::{ExecuteStatement, PrepareStatement};
use crate::types::DataType;

impl Cassie {
    pub(super) fn execute_prepare(
        &self,
        session: &CassieSession,
        prepare: &PrepareStatement,
        raw_sql: &str,
        call: SessionObjectCall<'_>,
    ) -> Result<QueryResult, CassieError> {
        let (exists, count) = session.with_session_objects(|objects| {
            (
                objects.prepared(&prepare.name).is_some(),
                objects.prepared_count(),
            )
        });
        if exists {
            return Err(SessionObjectError::duplicate_prepared(&prepare.name).into());
        }
        if count >= MAX_PREPARED_PER_SESSION {
            return Err(CassieError::ResourceLimit(
                "prepared statement limit exceeded".to_string(),
            ));
        }
        let parameter_count =
            prepare
                .parameter_types
                .len()
                .max(crate::sql::parser::max_positional_parameter(
                    &prepare.statement_sql,
                ));
        let mut parameter_types = prepare.parameter_types.clone();
        parameter_types.resize(parameter_count, String::new());
        let parameter_oids = parameter_types
            .iter()
            .map(|name| parameter_oid(name))
            .collect::<Result<Vec<_>, _>>()?;
        let parsed = (*prepare.statement).clone();
        let fingerprint = crate::runtime::sql_fingerprint(&parsed);
        let columns = if call.wire_output {
            self.describe_pgwire_parsed_statement_for_session(
                session,
                parsed,
                fingerprint,
                &parameter_oids,
            )?
        } else {
            self.describe_parsed_statement_for_session(
                session,
                parsed,
                fingerprint,
                &parameter_oids,
            )?
        };
        let output = command_result("PREPARE", call.controls)?;
        session.with_session_objects(|objects| {
            objects.insert_prepared(
                prepare.name.clone(),
                SessionPrepared {
                    statement_text: raw_sql.to_string(),
                    body_sql: prepare.statement_sql.clone(),
                    parameter_types,
                    result_types: columns.into_iter().map(|column| column.data_type).collect(),
                    wire_id: None,
                    prepare_time: crate::types::extended::temporal::session::current_timestamp(),
                },
            );
        });
        Ok(output)
    }

    pub(super) fn execute_prepared_sql(
        &self,
        session: &CassieSession,
        execute: &ExecuteStatement,
        call: SessionObjectCall<'_>,
    ) -> Result<QueryResult, CassieError> {
        let parsed = bound_statement(session, execute)?;
        let fingerprint = crate::runtime::sql_fingerprint(&parsed);
        self.execute_bound_statement(
            session,
            parsed,
            fingerprint,
            (Vec::new(), &[], call.wire_output),
            call.mode,
            call.controls,
        )
    }

    pub(super) fn describe_prepared_sql(
        &self,
        session: &CassieSession,
        execute: &ExecuteStatement,
        wire_output: bool,
    ) -> Result<Vec<crate::executor::ColumnMeta>, CassieError> {
        let parsed = bound_statement(session, execute)?;
        let fingerprint = crate::runtime::sql_fingerprint(&parsed);
        if wire_output {
            self.describe_pgwire_parsed_statement_for_session(session, parsed, fingerprint, &[])
        } else {
            self.describe_parsed_statement_for_session(session, parsed, fingerprint, &[])
        }
    }

    pub(super) fn execute_deallocate(
        session: &CassieSession,
        name: Option<&str>,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        let output = command_result(
            if name.is_some() {
                "DEALLOCATE"
            } else {
                "DEALLOCATE ALL"
            },
            controls,
        )?;
        session.with_session_objects(|objects| {
            let Some(name) = name else {
                objects.clear_prepared();
                return Ok(());
            };
            objects
                .remove_prepared(name)
                .map(|_| ())
                .ok_or_else(|| SessionObjectError::missing_prepared(name))
        })?;
        Ok(output)
    }
}

/// The OID a declared parameter type binds as; an undeclared slot is
/// inferred from its use.
fn parameter_oid(name: &str) -> Result<i32, CassieError> {
    if name.is_empty() {
        return Ok(0);
    }
    let data_type = DataType::parse_sql(name).map_err(CassieError::Parse)?;
    i32::try_from(data_type.type_oid())
        .map_err(|_| CassieError::Parse(format!("type \"{name}\" has no wire identity")))
}

/// The prepared body with each `$n` replaced by its argument coerced to the
/// declared parameter type, as PostgreSQL evaluates `EXECUTE` arguments.
fn bound_statement(
    session: &CassieSession,
    execute: &ExecuteStatement,
) -> Result<crate::sql::ast::ParsedStatement, CassieError> {
    let prepared = session
        .with_session_objects(|objects| objects.prepared(&execute.name).cloned())
        .ok_or_else(|| SessionObjectError::missing_prepared(&execute.name))?;
    if execute.arguments.len() != prepared.parameter_types.len() {
        return Err(SessionObjectError::wrong_parameter_count(&execute.name).into());
    }
    let sql = crate::sql::parser::substitute_positional_parameters(&prepared.body_sql, |index| {
        let argument = execute.arguments.get(index.checked_sub(1)?)?;
        let declared = prepared.parameter_types.get(index - 1)?;
        Some(if declared.is_empty() {
            format!("({argument})")
        } else {
            format!("CAST(({argument}) AS {declared})")
        })
    });
    Ok(crate::sql::parser::parse_statement(&sql)?)
}

/// `pg_prepared_statements.parameter_types` text for one declared slot.
pub(crate) fn parameter_type_display(name: &str) -> String {
    if name.is_empty() {
        return "text".to_string();
    }
    let display = DataType::parse_sql(name).map_or_else(
        |_| name.to_ascii_lowercase(),
        |data_type| data_type.type_name(),
    );
    match display.as_str() {
        "int" => "integer".to_string(),
        "float" => "double precision".to_string(),
        _ => display,
    }
}
