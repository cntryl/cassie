//! Shared checked SELECT semantics before execution, description and EXPLAIN shortcuts.

use crate::app::{Cassie, CassieError, CassieSession};
use crate::catalog::FunctionInputScope;
use crate::runtime::QueryExecutionControls;
use crate::sql::{
    checked_parameter_types_in_scope, AdmittedParameterTypes, ParameterAdmissionError,
    ParsedStatement, PrivateParameterTypeInput, QueryStatement,
};

pub(in crate::app) struct BoundParameterTypes {
    effective: Vec<i32>,
    admitted: Option<AdmittedParameterTypes>,
}

impl BoundParameterTypes {
    pub(in crate::app) fn types(&self) -> &[i32] {
        self.admitted
            .as_ref()
            .map_or(self.effective.as_slice(), AdmittedParameterTypes::types)
    }

    pub(in crate::app) fn is_admitted(&self) -> bool {
        self.admitted.is_some()
    }
}

pub(in crate::app) fn prepare_bound_parameters(
    cassie: &Cassie,
    session: &CassieSession,
    statement: &mut ParsedStatement,
    params: &mut [crate::types::Value],
    declared_oids: &[i32],
    controls: &QueryExecutionControls,
) -> Result<(BoundParameterTypes, bool), CassieError> {
    let effective = super::effective_parameter_type_oids(params, declared_oids);
    let admitted = admit_parameter_types(
        cassie,
        Some(session),
        statement,
        PrivateParameterTypeInput::Embedded {
            effective: &effective,
            values: params,
        },
        controls,
    )?;
    super::canonicalize_string_parameters(statement, &cassie.catalog, params);
    let bound =
        crate::sql::pagination::resolve_statement(statement, params, declared_oids, controls)?;
    Ok((
        BoundParameterTypes {
            effective,
            admitted,
        },
        bound,
    ))
}

pub(in crate::app) fn admit_parameter_types(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &ParsedStatement,
    input: PrivateParameterTypeInput<'_>,
    controls: &QueryExecutionControls,
) -> Result<Option<AdmittedParameterTypes>, CassieError> {
    let mut query = &statement.statement;
    while let QueryStatement::Explain(explain) = query {
        query = &explain.statement.statement;
    }
    if !matches!(query, QueryStatement::Select(_)) {
        // This slice preserves the existing embedded DML and command validation paths.
        return Ok(None);
    }
    let result = match session {
        Some(session) => session.with_search_path(|search_path| {
            let scope = FunctionInputScope {
                database: session
                    .current_database()
                    .unwrap_or(&cassie.default_database),
                search_path,
                user: &session.user,
                enforce_database_scope: cassie.database_catalog_enforced(),
            };
            checked_parameter_types_in_scope(
                statement,
                input,
                &cassie.catalog,
                controls,
                Some(&scope),
            )
        }),
        None => checked_parameter_types_in_scope(statement, input, &cassie.catalog, controls, None),
    };
    result.map(Some).map_err(project_error)
}

fn project_error(error: ParameterAdmissionError) -> CassieError {
    match error {
        ParameterAdmissionError::Memory(message) => CassieError::ResourceLimit(message),
        ParameterAdmissionError::Cancelled => CassieError::QueryCancelled,
        ParameterAdmissionError::Deadline => CassieError::DeadlineExceeded,
        ParameterAdmissionError::InvalidParam(message)
        | ParameterAdmissionError::LegacyConflict(message) => CassieError::Planner(message),
        ParameterAdmissionError::InvalidBooleanLiteral {
            input,
            _reservation: reservation,
        } => {
            let error =
                CassieError::Execution(format!("invalid input syntax for type boolean: {input:?}"));
            drop(input);
            drop(reservation);
            error
        }
        ParameterAdmissionError::NonAggregateModifier {
            message,
            _reservation: reservation,
        } => {
            let error = CassieError::Execution(message);
            drop(reservation);
            error
        }
        // The existing public error shape has no ownership/detail/SQLSTATE carrier.
        ParameterAdmissionError::Semantic(diagnostic) if diagnostic.code() == "0A000" => {
            CassieError::Unsupported(diagnostic.message())
        }
        ParameterAdmissionError::Semantic(diagnostic) if diagnostic.uses_execution_error() => {
            CassieError::Execution(diagnostic.message())
        }
        ParameterAdmissionError::Semantic(diagnostic) => CassieError::Planner(diagnostic.message()),
    }
}
