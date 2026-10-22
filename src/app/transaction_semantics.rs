use crate::catalog::Catalog;
use crate::sql::ast::{QueryStatement, StatementFamily};

use super::{CassieError, CassieSession};

pub(crate) fn ensure_supported_transaction_semantics(
    catalog: &Catalog,
    session: &CassieSession,
    statement: &QueryStatement,
) -> Result<(), CassieError> {
    if !read_only_control(statement) {
        session.ensure_transaction_writable()?;
    }
    if !session.is_transaction_active() {
        return Ok(());
    }

    if matches!(statement, QueryStatement::Copy(_)) {
        return reject_active_transaction_statement(
            session,
            "COPY is not supported inside an active transaction",
        );
    }

    if statement.family() != StatementFamily::Runtime
        && (session.is_implicit_transaction() || !transactional_ddl_supported(catalog, statement))
    {
        return reject_active_transaction_statement(
            session,
            "this DDL or catalog operation is not supported inside an active transaction",
        );
    }

    Ok(())
}

fn transactional_ddl_supported(catalog: &Catalog, statement: &QueryStatement) -> bool {
    match statement {
        QueryStatement::CreateTable(_)
        | QueryStatement::DropTable(_)
        | QueryStatement::AlterTable(_)
        | QueryStatement::CreateSchema(_)
        | QueryStatement::DropSchema(_)
        | QueryStatement::AlterSchema(_) => true,
        QueryStatement::CreateIndex(index) => index.kind == crate::catalog::IndexKind::Scalar,
        QueryStatement::DropIndex(index) => catalog
            .all_indexes_snapshot()
            .into_iter()
            .find(|stored| {
                crate::catalog::name_matches(&stored.name, &index.name)
                    && (index.table.is_empty()
                        || crate::catalog::name_matches(&stored.collection, &index.table))
            })
            .is_none_or(|stored| stored.kind == crate::catalog::IndexKind::Scalar),
        _ => false,
    }
}

fn reject_active_transaction_statement(
    session: &CassieSession,
    message: &str,
) -> Result<(), CassieError> {
    session.mark_transaction_failed();
    Err(CassieError::Unsupported(message.to_string()))
}

fn read_only_control(statement: &QueryStatement) -> bool {
    match statement {
        // A data-modifying WITH query needs a writable transaction.
        QueryStatement::Select(select) => !select
            .ctes
            .iter()
            .any(crate::sql::binder::cte_bodies::cte_is_modifying),
        QueryStatement::Show(_)
        | QueryStatement::Set(_)
        | QueryStatement::Transaction(_)
        | QueryStatement::SessionObject(_)
        | QueryStatement::CallProcedure(_) => true,
        QueryStatement::Explain(explain) => {
            !explain.analyze || read_only_control(&explain.statement.statement)
        }
        _ => false,
    }
}
