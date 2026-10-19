//! `DECLARE`, `FETCH`, `MOVE` and `CLOSE`.
//!
//! A cursor materializes its query when declared, which gives every cursor
//! PostgreSQL's `INSENSITIVE` behavior. A cursor is scrollable only when
//! declared `SCROLL`; PostgreSQL may also allow backward scans of a cursor
//! declared without either option when its plan supports them, while Cassie
//! deterministically treats that default as `NO SCROLL`.

use super::fetch_plan::plan_fetch;
use super::{command_result, SessionObjectCall, SessionObjectError};
use crate::app::{
    Cassie, CassieError, CassieSession, CursorOptions, QueryExecutionControls, QueryResult,
    SessionCursor, TransactionErrorKind, MAX_CURSORS_PER_SESSION,
};
use crate::sql::ast::{DeclareCursorStatement, FetchStatement};

impl Cassie {
    pub(super) fn execute_declare(
        &self,
        session: &CassieSession,
        declare: &DeclareCursorStatement,
        raw_sql: &str,
        call: SessionObjectCall<'_>,
    ) -> Result<QueryResult, CassieError> {
        let in_block = session.is_transaction_active() && !session.is_implicit_transaction();
        if !declare.hold && !in_block {
            return Err(CassieError::Transaction {
                kind: TransactionErrorKind::NoActiveTransaction,
                message: "DECLARE CURSOR can only be used in transaction blocks".to_string(),
            });
        }
        let (exists, count) = session.with_session_objects(|objects| {
            (
                objects.cursor(&declare.name).is_some(),
                objects.cursor_count(),
            )
        });
        if exists {
            return Err(SessionObjectError::duplicate_cursor(&declare.name).into());
        }
        if count >= MAX_CURSORS_PER_SESSION {
            return Err(CassieError::ResourceLimit(
                "cursor limit exceeded".to_string(),
            ));
        }
        let parsed = (*declare.query).clone();
        let fingerprint = crate::runtime::sql_fingerprint(&parsed);
        let result = self.execute_bound_statement(
            session,
            parsed,
            fingerprint,
            (Vec::new(), &[], call.wire_output),
            call.mode,
            call.controls,
        )?;
        let output = command_result("DECLARE CURSOR", call.controls)?;
        session.with_session_objects(|objects| {
            objects.insert_cursor(
                declare.name.clone(),
                SessionCursor {
                    statement_text: raw_sql.to_string(),
                    result,
                    position: 0,
                    options: CursorOptions {
                        scroll: declare.scroll == Some(true),
                        hold: declare.hold,
                        binary: declare.binary,
                    },
                    creation_time: crate::types::extended::temporal::session::current_timestamp(),
                    in_transaction: session.is_transaction_active(),
                    sequence: 0,
                },
            );
        });
        Ok(output)
    }

    pub(super) fn execute_fetch(
        session: &CassieSession,
        fetch: &FetchStatement,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        let (result, rows) = session.with_session_objects(|objects| {
            let cursor = objects
                .cursor_mut(&fetch.cursor)
                .ok_or_else(|| SessionObjectError::missing_cursor(&fetch.cursor))?;
            let plan = plan_fetch(
                fetch.direction,
                cursor.position,
                cursor.result.row_count(),
                cursor.options.scroll,
            )?;
            cursor.position = plan.position;
            Ok::<_, SessionObjectError>((cursor.result.clone(), plan.rows))
        })?;
        if fetch.move_only {
            return command_result(&format!("MOVE {}", rows.len()), controls);
        }
        let mut output = QueryResult::try_builder(controls)?;
        for column in result.columns() {
            output.try_push_column(column)?;
        }
        for index in &rows {
            output.try_push_row(&result.rows()[*index])?;
        }
        output.try_set_command(&format!("FETCH {}", rows.len()))?;
        output.finish()
    }

    pub(super) fn describe_fetch(
        session: &CassieSession,
        fetch: &FetchStatement,
    ) -> Result<Vec<crate::executor::ColumnMeta>, CassieError> {
        session.with_session_objects(|objects| {
            objects
                .cursor(&fetch.cursor)
                .map(|cursor| cursor.result.columns().to_vec())
                .ok_or_else(|| SessionObjectError::missing_cursor(&fetch.cursor).into())
        })
    }

    pub(super) fn execute_close(
        session: &CassieSession,
        name: Option<&str>,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        let output = command_result(
            if name.is_some() {
                "CLOSE CURSOR"
            } else {
                "CLOSE CURSOR ALL"
            },
            controls,
        )?;
        session.with_session_objects(|objects| {
            let Some(name) = name else {
                objects.clear_cursors();
                return Ok(());
            };
            objects
                .remove_cursor(name)
                .map(|_| ())
                .ok_or_else(|| SessionObjectError::missing_cursor(name))
        })?;
        Ok(output)
    }
}
