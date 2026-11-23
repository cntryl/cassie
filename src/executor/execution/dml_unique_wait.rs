//! Shared transaction unique-key arbitration for atomic DML commands.

use super::{
    check_timeout, Cassie, CassieSession, QueryError, QueryExecutionControls, QueryResult,
};

#[cfg(test)]
#[path = "dml_unique_wait_tests.rs"]
mod tests;

pub(super) fn execute_atomic_dml(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    controls: &QueryExecutionControls,
    wait_for_unique_owner: bool,
    include_preimages: bool,
    mut execute: impl FnMut(
        &Cassie,
        &CassieSession,
        &QueryExecutionControls,
    ) -> Result<QueryResult, QueryError>,
) -> Result<QueryResult, QueryError> {
    let fallback = session
        .is_none()
        .then(|| cassie.create_session("root", None));
    let session = session.or(fallback.as_ref());
    let collections = cassie.referential_write_collections(table);
    let mut pending_attempt: Option<PendingUniqueDml> = None;
    loop {
        let attempt = cassie.midge.with_collection_write_gates(&collections, || {
            if let Some(pending) = pending_attempt.take() {
                let Some(original) =
                    session.filter(|session| pending.batch.matches_current_owner(session))
                else {
                    return Err(QueryError::from(crate::app::CassieError::Execution(
                        "transaction changed while DML waited for a unique key".to_owned(),
                    )));
                };
                return attempt_unique_dml_publication(
                    cassie,
                    original,
                    controls,
                    pending,
                    wait_for_unique_owner,
                );
            }

            let batch = cassie
                .new_statement_batch(session)
                .map_err(QueryError::from)?;
            // The new owner paths use READ COMMITTED arbitration. Repeatable
            // read mutations retain their row-conflict check at native commit.
            let arbitrate = !include_preimages || !batch.session().is_repeatable_read_write();
            batch.session().set_defer_unique_wait_duplicate(arbitrate);
            let statement_controls = super::super::entrypoints::working_statement_read_controls(
                cassie,
                session,
                batch.session(),
                controls,
            )?;
            let working_cassie = cassie.with_statement_metadata_writer(batch.session())?;
            let result = execute(&working_cassie, batch.session(), &statement_controls)?;
            check_timeout(controls)?;

            let Some(original) = session else {
                return Err(QueryError::from(crate::app::CassieError::Execution(
                    "DML unique-key ownership requires a session".to_owned(),
                )));
            };
            if !batch.matches_current_owner(original) {
                return Err(QueryError::from(crate::app::CassieError::Execution(
                    "session transaction changed while DML was executing".to_owned(),
                )));
            }
            if !arbitrate {
                cassie.publish_statement_batch(Some(original), batch, controls)?;
                return Ok(AtomicDmlAttempt::Published(result));
            }
            let owner = if batch.has_enclosing_transaction() {
                batch.unique_owner_id()
            } else {
                uuid::Uuid::new_v4()
            };
            let catalog = batch
                .session()
                .transaction_catalog_overlay()
                .unwrap_or_else(|| cassie.catalog.clone());
            let (keys, candidate_keys) = {
                let _read = crate::midge::adapter::StatementReadScope::enter(None);
                let _overlay = crate::app::SessionReadScope::enter(None);
                crate::app::collect_statement_unique_keys(
                    &catalog,
                    &cassie.midge,
                    batch.session(),
                    &batch.changed_row_ids()?,
                    include_preimages,
                )?
            };
            let pending = PendingUniqueDml {
                owner,
                batch,
                result,
                keys,
                candidate_keys,
                resolved_keys: std::collections::BTreeSet::new(),
            };
            attempt_unique_dml_publication(
                cassie,
                original,
                controls,
                pending,
                wait_for_unique_owner,
            )
        })?;

        match attempt {
            AtomicDmlAttempt::Published(result) => return Ok(result),
            AtomicDmlAttempt::Wait {
                pending,
                key,
                owner,
            } => {
                let pending = *pending;
                cassie
                    .transaction_unique_waits
                    .wait_for_key(pending.owner, owner, &key, controls)
                    .map_err(QueryError::from)?;
                pending_attempt = Some(pending);
            }
        }
    }
}

struct PendingUniqueDml {
    owner: uuid::Uuid,
    batch: crate::app::StatementMutationBatch,
    result: QueryResult,
    keys: std::collections::BTreeSet<crate::app::UniqueTransactionKey>,
    candidate_keys: std::collections::BTreeSet<crate::app::UniqueTransactionKey>,
    resolved_keys: std::collections::BTreeSet<crate::app::UniqueTransactionKey>,
}

enum AtomicDmlAttempt {
    Published(QueryResult),
    Wait {
        pending: Box<PendingUniqueDml>,
        key: crate::app::UniqueTransactionKey,
        owner: uuid::Uuid,
    },
}

fn attempt_unique_dml_publication(
    cassie: &Cassie,
    original: &CassieSession,
    controls: &QueryExecutionControls,
    mut pending: PendingUniqueDml,
    wait_for_unique_owner: bool,
) -> Result<AtomicDmlAttempt, QueryError> {
    #[cfg(test)]
    tests::before_unique_publication();
    let waits = &cassie.transaction_unique_waits;
    let claim =
        original.with_unique_statement_publication(&pending.batch, controls, |publish| {
            claim_unique_dml(cassie, &pending, wait_for_unique_owner, publish)
        })?;
    if let crate::app::UniqueKeyClaim::Wait { key, owner } = claim {
        pending.resolved_keys.insert(key.clone());
        return Ok(AtomicDmlAttempt::Wait {
            pending: Box::new(pending),
            key,
            owner,
        });
    }
    if !pending.batch.has_enclosing_transaction() {
        let publication = cassie.publish_statement_batch(Some(original), pending.batch, controls);
        waits.release_owner(pending.owner);
        publication?;
    }
    Ok(AtomicDmlAttempt::Published(pending.result))
}

fn claim_unique_dml(
    cassie: &Cassie,
    pending: &PendingUniqueDml,
    wait_for_unique_owner: bool,
    publish: &mut dyn FnMut() -> Result<(), crate::app::CassieError>,
) -> Result<crate::app::UniqueKeyClaim, crate::app::CassieError> {
    use crate::app::UniqueKeyClaim;

    // A modifying WITH statement may still own a source snapshot. Its row
    // expressions retain that snapshot, while arbitration checks current data.
    let _read = crate::midge::adapter::StatementReadScope::enter(None);
    let _overlay = crate::app::SessionReadScope::enter(None);
    let waits = &cassie.transaction_unique_waits;
    let catalog = pending
        .batch
        .session()
        .transaction_catalog_overlay()
        .unwrap_or_else(|| cassie.catalog.clone());
    for key in &pending.resolved_keys {
        crate::app::validate_unique_key_candidate(
            cassie,
            &catalog,
            &cassie.midge,
            pending.batch.session(),
            key,
        )?;
    }

    let previous_keys = waits.snapshot_owner_keys(pending.owner);
    let claim = if wait_for_unique_owner {
        waits.claim_keys(pending.owner, &pending.keys)
    } else {
        waits.claim_keys_without_wait(pending.owner, &pending.keys)
    }?;
    if matches!(&claim, UniqueKeyClaim::Wait { .. }) {
        return Ok(claim);
    }

    // Validation may have deferred a committed duplicate to a registered
    // transaction owner. Recheck every staged candidate after claiming, even
    // when that owner resolved before claim_keys found a blocker.
    pending
        .batch
        .session()
        .set_defer_unique_wait_duplicate(false);
    let recheck_result = pending.candidate_keys.iter().try_for_each(|key| {
        crate::app::validate_unique_key_candidate(
            cassie,
            &catalog,
            &cassie.midge,
            pending.batch.session(),
            key,
        )
    });
    if let Err(error) = recheck_result.and_then(|()| publish()) {
        waits.restore_owner_keys(pending.owner, &previous_keys)?;
        return Err(error);
    }
    Ok(claim)
}
