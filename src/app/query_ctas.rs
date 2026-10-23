use std::mem::size_of;

use super::{
    Cassie, CassieError, CassieSession, ExecutionMode, QueryExecutionControls, QueryResult,
    QueryStatement, Value,
};
use crate::sql::ast::{
    CreateTableAsClause, CreateTableStatement, FieldDefinition, ParsedStatement,
    SessionObjectStatement,
};

const CTAS_SAVEPOINT_NAME: &str = "cassie_ctas_statement";

impl Cassie {
    pub(super) fn execute_create_table_as(
        &self,
        session: &CassieSession,
        statement: &CreateTableStatement,
        raw_sql: &str,
        parameters: (Vec<Value>, &[i32], bool),
        mode: ExecutionMode,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        let Some(as_query) = statement.as_query.as_ref() else {
            return Err(CassieError::Execution(
                "CREATE TABLE AS query is missing".to_string(),
            ));
        };
        let mut scoped = self.clone();
        if let Some(catalog) = session.transaction_catalog_overlay() {
            scoped.catalog = catalog;
        }
        let parsed_target = ParsedStatement {
            raw_sql: raw_sql.to_string(),
            statement: QueryStatement::CreateTable(statement.clone()),
        };
        let bound_target = crate::sql::binder::bind_with_context(
            parsed_target,
            &scoped.catalog,
            &scoped.binding_context_for_session(Some(session)),
        )?;
        let QueryStatement::CreateTable(bound_target) = bound_target.statement.statement else {
            return Err(CassieError::Execution(
                "expected a bound CREATE TABLE statement".to_string(),
            ));
        };
        if statement.if_not_exists && scoped.catalog.relation_exists(&bound_target.table) {
            return command_result("SELECT 0", controls);
        }

        let transaction_active = session.is_transaction_active();
        let owns_transaction = !transaction_active;
        let savepoint = transaction_active && !session.is_implicit_transaction();
        if owns_transaction {
            session.begin_transaction(None)?;
        } else if savepoint {
            session.create_savepoint(CTAS_SAVEPOINT_NAME)?;
        }

        let result = scoped.execute_create_table_as_in_transaction(
            session, statement, as_query, parameters, mode, controls,
        );
        match result {
            Ok(result) => {
                if owns_transaction {
                    if let Err(error) = scoped.commit_transaction(session) {
                        session.rollback_transaction();
                        return Err(error);
                    }
                } else if savepoint {
                    session.release_savepoint(CTAS_SAVEPOINT_NAME)?;
                }
                Ok(result)
            }
            Err(error) => {
                if owns_transaction {
                    session.rollback_transaction();
                } else if savepoint {
                    session.rollback_to_savepoint(CTAS_SAVEPOINT_NAME)?;
                    session.release_savepoint(CTAS_SAVEPOINT_NAME)?;
                }
                Err(error)
            }
        }
    }

    fn execute_create_table_as_in_transaction(
        &self,
        session: &CassieSession,
        target: &CreateTableStatement,
        as_query: &CreateTableAsClause,
        parameters: (Vec<Value>, &[i32], bool),
        mode: ExecutionMode,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        let (source_parameters, declared_oids, wire_output) = parameters;
        let source_fingerprint = crate::runtime::sql_fingerprint(&as_query.query);
        let columns = self.describe_parsed_statement_for_session(
            session,
            as_query.query.as_ref().clone(),
            source_fingerprint,
            declared_oids,
        )?;
        if !as_query.column_names.is_empty() && as_query.column_names.len() != columns.len() {
            return Err(CassieError::Planner(format!(
                "CREATE TABLE AS column list has {} names but query returns {} columns",
                as_query.column_names.len(),
                columns.len()
            )));
        }

        let fields = columns
            .iter()
            .enumerate()
            .map(|(index, column)| {
                let name = as_query
                    .column_names
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| {
                        if column.name.is_empty() {
                            format!("column{}", index + 1)
                        } else {
                            column.name.clone()
                        }
                    });
                let data_type = crate::types::DataType::parse_sql(&column.data_type)
                    .map_err(CassieError::Parse)?;
                Ok(FieldDefinition {
                    name,
                    data_type,
                    constraints: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>, CassieError>>()?;
        let create_table = ParsedStatement {
            raw_sql: format!("CREATE TABLE {}", target.table),
            statement: QueryStatement::CreateTable(CreateTableStatement {
                table: target.table.clone(),
                fields,
                if_not_exists: target.if_not_exists,
                storage_mode: target.storage_mode,
                as_query: None,
                like: None,
            }),
        };
        self.execute_bound_statement(
            session,
            create_table.clone(),
            crate::runtime::sql_fingerprint(&create_table),
            (Vec::new(), &[], wire_output),
            mode,
            controls,
        )?;
        let row_count = if as_query.with_data {
            if matches!(
                &as_query.query.statement,
                QueryStatement::SessionObject(SessionObjectStatement::Execute(_))
            ) {
                self.execute_prepared_ctas_rows(
                    session,
                    target,
                    as_query,
                    (source_parameters, declared_oids, wire_output),
                    mode,
                    controls,
                )?
            } else {
                let insert_sql = format!("INSERT INTO {} {}", target.table, as_query.query.raw_sql);
                let insert = crate::sql::parser::parse_statement(&insert_sql)?;
                let inserted = self.execute_bound_statement(
                    session,
                    insert.clone(),
                    crate::runtime::sql_fingerprint(&insert),
                    (source_parameters, declared_oids, wire_output),
                    mode,
                    controls,
                )?;
                affected_rows(&inserted)
            }
        } else {
            0
        };

        command_result(&format!("SELECT {row_count}"), controls)
    }

    fn execute_prepared_ctas_rows(
        &self,
        session: &CassieSession,
        target: &CreateTableStatement,
        as_query: &CreateTableAsClause,
        parameters: (Vec<Value>, &[i32], bool),
        mode: ExecutionMode,
        controls: &QueryExecutionControls,
    ) -> Result<u64, CassieError> {
        let (source_parameters, declared_oids, wire_output) = parameters;
        let query = as_query.query.as_ref().clone();
        let rows = self.execute_bound_statement(
            session,
            query.clone(),
            crate::runtime::sql_fingerprint(&query),
            (source_parameters, declared_oids, wire_output),
            mode,
            controls,
        )?;
        if rows.columns().is_empty() {
            return Err(CassieError::Execution(
                "CREATE TABLE AS EXECUTE returned no columns".to_string(),
            ));
        }
        let placeholders = (1..=rows.columns().len())
            .map(|index| format!("${index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let insert_sql = format!("INSERT INTO {} VALUES ({placeholders})", target.table);
        let insert = crate::sql::parser::parse_statement(&insert_sql)?;
        let fingerprint = crate::runtime::sql_fingerprint(&insert);
        let mut inserted = 0_u64;
        for row in rows.rows() {
            if row.len() != rows.columns().len() {
                return Err(CassieError::Execution(
                    "CREATE TABLE AS EXECUTE returned a row with the wrong column count"
                        .to_string(),
                ));
            }
            let bytes = row_memory_bytes(row)?;
            let _row_memory = controls.reserve_query_memory(bytes)?;
            self.execute_bound_statement(
                session,
                insert.clone(),
                fingerprint,
                (row.clone(), &[], false),
                mode,
                controls,
            )?;
            inserted = inserted
                .checked_add(1)
                .ok_or_else(|| CassieError::Execution("row count overflow".to_string()))?;
        }
        Ok(inserted)
    }
}

fn command_result(
    command: &str,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, CassieError> {
    let mut result = QueryResult::try_builder(controls)?;
    result.try_set_command(command)?;
    result.finish()
}

fn affected_rows(result: &QueryResult) -> u64 {
    result
        .command()
        .split_whitespace()
        .last()
        .and_then(|count| count.parse().ok())
        .unwrap_or(0)
}

fn row_memory_bytes(row: &[Value]) -> Result<usize, CassieError> {
    let inline_bytes = row
        .len()
        .checked_mul(size_of::<Value>())
        .and_then(|bytes| bytes.checked_add(size_of::<Vec<Value>>()))
        .ok_or_else(|| CassieError::Execution("CTAS row size overflow".to_string()))?;
    row.iter().try_fold(inline_bytes, |total, value| {
        crate::executor::retained_memory::add(
            total,
            crate::executor::retained_memory::value_clone_bytes(value)?,
        )
    })
}
