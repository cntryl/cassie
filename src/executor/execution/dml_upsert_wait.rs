//! Claim the selected arbiter before executing an upsert action or RETURNING.
use super::{
    Cassie, CassieSession, FunctionMeta, HashMap, QueryError, QueryExecutionControls, QueryResult,
    Value,
};
use crate::app::{UniqueKeyClaim, UniqueTransactionKey};
use crate::sql::ast::{InsertConflictAction, InsertStatement};
use crate::types::DataType;
use std::collections::BTreeSet;

pub(super) fn arbiter(cassie: &Cassie, statement: &InsertStatement) -> Option<String> {
    let clause = statement.on_conflict.as_ref()?;
    if clause.constraint.is_some()
        || clause.target_predicate.is_some()
        || !clause.target_expressions.is_empty()
        || !clause.update_row_subqueries.is_empty()
    {
        return None;
    }
    let [field] = clause.target_fields.as_slice() else {
        return None;
    };
    if let InsertConflictAction::DoUpdate { assignments, .. } = &clause.action {
        if assignments.iter().any(|(name, _)| name == field) {
            return None;
        }
    }
    let constraints = cassie.catalog.get_constraints(&statement.table);
    if !constraints.iter().any(|constraint| {
        constraint.field == *field
            && crate::catalog::enforces_single_column_uniqueness(constraint, &constraints)
    }) {
        return None;
    }
    let schema = cassie.catalog.get_schema(&statement.table)?;
    let kind = &schema
        .fields
        .iter()
        .find(|column| column.name == *field)?
        .data_type;
    if !matches!(
        kind,
        DataType::SmallInt
            | DataType::Int
            | DataType::BigInt
            | DataType::Boolean
            | DataType::Text
            | DataType::Varchar { .. }
            | DataType::Uuid
            | DataType::Bytea
    ) || !cassie
        .catalog
        .list_vector_indexes(&statement.table)
        .is_empty()
    {
        return None;
    }
    Some(field.clone())
}

#[derive(Clone, Copy)]
pub(super) struct UpsertRequest<'a> {
    pub(super) cassie: &'a Cassie,
    pub(super) session: Option<&'a CassieSession>,
    pub(super) statement: &'a InsertStatement,
    pub(super) params: &'a [Value],
    pub(super) functions: &'a HashMap<String, FunctionMeta>,
    pub(super) controls: &'a QueryExecutionControls,
    pub(super) arbiter: &'a str,
}

#[derive(Default)]
struct PendingUpsert {
    prepared: Option<super::dml::WaitingInsert>,
    keys: Option<BTreeSet<UniqueTransactionKey>>,
    owner: Option<uuid::Uuid>,
    previous: Option<BTreeSet<UniqueTransactionKey>>,
}

pub(super) fn execute(request: UpsertRequest<'_>) -> Result<QueryResult, QueryError> {
    let fallback = request
        .session
        .is_none()
        .then(|| request.cassie.create_session("root", None));
    let request = UpsertRequest {
        session: request.session.or(fallback.as_ref()),
        ..request
    };
    let collections = request
        .cassie
        .referential_write_collections(&request.statement.table);
    let mut pending = PendingUpsert::default();
    loop {
        let attempt = request
            .cassie
            .midge
            .with_collection_write_gates(&collections, || pending.attempt(request))?;
        match attempt {
            Attempt::Published(result) => return Ok(result),
            Attempt::Wait {
                key,
                blocker,
                owner,
            } => request.cassie.transaction_unique_waits.wait_for_key(
                owner,
                blocker,
                &key,
                request.controls,
            )?,
        }
    }
}

impl PendingUpsert {
    fn attempt(&mut self, request: UpsertRequest<'_>) -> Result<Attempt, QueryError> {
        let UpsertRequest {
            cassie,
            session,
            statement,
            params,
            functions,
            controls,
            arbiter,
        } = request;
        let Self {
            prepared,
            keys,
            owner,
            previous,
        } = self;
        controls.check_interruption()?;
        let batch = cassie.new_statement_batch(session)?;
        let current_owner = batch.session().transaction_unique_owner_id();
        if owner.is_some_and(|owner| owner != current_owner) {
            return Err(crate::app::CassieError::Execution(
                "transaction changed while upsert waited".into(),
            )
            .into());
        }
        *owner = Some(current_owner);
        if prepared.is_none() {
            *prepared = Some(super::dml::prepare_waiting_insert(
                cassie,
                batch.session(),
                statement,
                params,
                functions,
                controls,
            )?);
        }
        let keys = keys.get_or_insert_with(|| {
            crate::app::collect_upsert_unique_keys(
                &cassie.catalog,
                &cassie.midge,
                &statement.table,
                arbiter,
                prepared.as_ref().expect("prepared candidates").payloads(),
            )
        });
        let previous = previous.get_or_insert_with(|| {
            cassie
                .transaction_unique_waits
                .snapshot_owner_keys(current_owner)
        });
        match cassie
            .transaction_unique_waits
            .claim_keys(current_owner, keys)?
        {
            UniqueKeyClaim::Wait { key, owner } => {
                return Ok(Attempt::Wait {
                    key,
                    blocker: owner,
                    owner: current_owner,
                })
            }
            UniqueKeyClaim::Claimed => {}
        }
        let result: Result<QueryResult, QueryError> = (|| {
            let result = super::dml::execute_prepared_insert(
                cassie,
                batch.session(),
                statement,
                params,
                functions,
                controls,
                prepared.take().expect("prepared candidates"),
            )?;
            let actual =
                crate::app::collect_unique_keys(&cassie.catalog, &cassie.midge, batch.session());
            let mut retained = previous.clone();
            retained.extend(keys.intersection(&actual).cloned());
            cassie.publish_statement_batch(session, batch, controls)?;
            if session.is_some_and(CassieSession::is_transaction_active) {
                cassie
                    .transaction_unique_waits
                    .restore_owner_keys(current_owner, &retained)?;
            }
            Ok(result)
        })();
        if result.is_err() || !session.is_some_and(CassieSession::is_transaction_active) {
            cassie
                .transaction_unique_waits
                .restore_owner_keys(current_owner, previous)?;
        }
        result.map(Attempt::Published)
    }
}

enum Attempt {
    Published(QueryResult),
    Wait {
        key: UniqueTransactionKey,
        blocker: uuid::Uuid,
        owner: uuid::Uuid,
    },
}
