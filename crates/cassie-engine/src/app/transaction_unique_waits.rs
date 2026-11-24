//! Transaction scoped ownership for single column unique keys staged by INSERT or UPDATE.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

#[cfg(test)]
use parking_lot::MutexGuard;
use parking_lot::{Condvar, Mutex};
use uuid::Uuid;

#[cfg(test)]
use std::cell::RefCell;
#[cfg(test)]
use std::sync::mpsc::Sender;
#[cfg(test)]
use std::sync::OnceLock;

use crate::app::{Cassie, CassieError, CassieSession, TransactionErrorKind, TransactionRowChange};
use crate::catalog::{Catalog, FieldConstraint};
use crate::midge::adapter::Midge;
use crate::runtime::QueryExecutionControls;

#[cfg(test)]
static ROW_WAIT_OBSERVER: OnceLock<Mutex<Option<Sender<()>>>> = OnceLock::new();
#[cfg(test)]
static ROW_WAIT_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(test)]
thread_local! {
    static UNIQUE_WAIT_TEST_OBSERVER: RefCell<Option<Box<dyn FnOnce() + Send>>> = const { RefCell::new(None) };
}

#[cfg(test)]
struct UniqueWaitTestObserverGuard;

#[cfg(test)]
fn observe_next_unique_transaction_wait_for_test(
    observer: impl FnOnce() + Send + 'static,
) -> UniqueWaitTestObserverGuard {
    UNIQUE_WAIT_TEST_OBSERVER.with(|slot| {
        assert!(
            slot.borrow().is_none(),
            "unique wait observer already installed"
        );
        *slot.borrow_mut() = Some(Box::new(observer));
    });
    UniqueWaitTestObserverGuard
}

#[cfg(test)]
impl Drop for UniqueWaitTestObserverGuard {
    fn drop(&mut self) {
        UNIQUE_WAIT_TEST_OBSERVER.with(|slot| {
            let _ = slot.borrow_mut().take();
        });
    }
}

#[cfg(test)]
fn signal_unique_wait_test_observer() {
    UNIQUE_WAIT_TEST_OBSERVER.with(|slot| {
        if let Some(observer) = slot.borrow_mut().take() {
            observer();
        }
    });
}

#[cfg(test)]
struct RowWaitObserverGuard {
    _test_lock: MutexGuard<'static, ()>,
}

#[cfg(test)]
pub(crate) fn install_row_wait_observer(sender: Sender<()>) -> impl Drop {
    let test_lock = ROW_WAIT_TEST_LOCK.get_or_init(|| Mutex::new(())).lock();
    *ROW_WAIT_OBSERVER.get_or_init(|| Mutex::new(None)).lock() = Some(sender);
    RowWaitObserverGuard {
        _test_lock: test_lock,
    }
}

#[cfg(test)]
impl Drop for RowWaitObserverGuard {
    fn drop(&mut self) {
        if let Some(observer) = ROW_WAIT_OBSERVER.get() {
            *observer.lock() = None;
        }
    }
}

#[cfg(test)]
fn observe_row_wait() {
    if let Some(observer) = ROW_WAIT_OBSERVER.get() {
        if let Some(sender) = observer.lock().as_ref() {
            let _ = sender.send(());
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct UniqueTransactionKey {
    relation: String,
    constraint: String,
    field: String,
    value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UniqueKeyClaim {
    Claimed,
    Wait {
        key: UniqueTransactionKey,
        owner: Uuid,
        wait_id: Uuid,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RowTransactionKey {
    collection: String,
    id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RowKeyClaim {
    Claimed,
    Wait {
        collection: String,
        id: String,
        owner: Uuid,
        wait_id: Uuid,
    },
}

impl RowKeyClaim {
    pub(crate) fn into_wait(self) -> Option<(String, String, Uuid, Uuid)> {
        match self {
            Self::Claimed => None,
            Self::Wait {
                collection,
                id,
                owner,
                wait_id,
            } => Some((collection, id, owner, wait_id)),
        }
    }
}

#[derive(Debug, Default)]
struct RegistryState {
    owners: BTreeMap<UniqueTransactionKey, Uuid>,
    keys_by_owner: BTreeMap<Uuid, BTreeSet<UniqueTransactionKey>>,
    row_owners: BTreeMap<RowTransactionKey, Uuid>,
    rows_by_owner: BTreeMap<Uuid, BTreeSet<RowTransactionKey>>,
    waits_for: BTreeMap<Uuid, WaitEdge>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WaitEdge {
    blocker: Uuid,
    id: Uuid,
}

#[derive(Debug, Default)]
pub(crate) struct TransactionUniqueWaits {
    state: Mutex<RegistryState>,
    changed: Condvar,
}

impl TransactionUniqueWaits {
    pub(crate) fn waiting_count(&self) -> usize {
        self.state.lock().waits_for.len()
    }

    pub(crate) fn has_other_owner(&self, owner: Uuid, key: &UniqueTransactionKey) -> bool {
        self.state
            .lock()
            .owners
            .get(key)
            .is_some_and(|current| *current != owner)
    }

    pub(crate) fn claim_keys(
        &self,
        owner: Uuid,
        keys: &BTreeSet<UniqueTransactionKey>,
    ) -> Result<UniqueKeyClaim, CassieError> {
        self.claim_keys_with_wait(owner, keys, true)
    }

    pub(crate) fn claim_keys_without_wait(
        &self,
        owner: Uuid,
        keys: &BTreeSet<UniqueTransactionKey>,
    ) -> Result<UniqueKeyClaim, CassieError> {
        self.claim_keys_with_wait(owner, keys, false)
    }

    fn claim_keys_with_wait(
        &self,
        owner: Uuid,
        keys: &BTreeSet<UniqueTransactionKey>,
        wait: bool,
    ) -> Result<UniqueKeyClaim, CassieError> {
        let mut state = self.state.lock();
        state.waits_for.remove(&owner);
        if let Some((key, blocker)) = keys.iter().find_map(|key| {
            state
                .owners
                .get(key)
                .filter(|blocker| **blocker != owner)
                .map(|blocker| (key.clone(), *blocker))
        }) {
            if !wait {
                return Err(CassieError::Transaction {
                    kind: TransactionErrorKind::SerializationFailure,
                    message:
                        "unique key is owned by another active transaction; retry the transaction"
                            .to_owned(),
                });
            }
            if closes_wait_cycle(&state, owner, blocker) {
                return Err(deadlock_error());
            }
            let wait_id = Uuid::new_v4();
            state.waits_for.insert(
                owner,
                WaitEdge {
                    blocker,
                    id: wait_id,
                },
            );
            return Ok(UniqueKeyClaim::Wait {
                key,
                owner: blocker,
                wait_id,
            });
        }

        for key in keys {
            state.owners.insert(key.clone(), owner);
        }
        state
            .keys_by_owner
            .entry(owner)
            .or_default()
            .extend(keys.iter().cloned());
        Ok(UniqueKeyClaim::Claimed)
    }

    pub(crate) fn wait_for_key(
        &self,
        waiter: Uuid,
        owner: Uuid,
        key: &UniqueTransactionKey,
        wait_id: Uuid,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        let mut state = self.state.lock();
        loop {
            if let Err(error) = controls.check_interruption() {
                clear_wait_edge(&mut state, waiter, owner, wait_id);
                return Err(error);
            }
            if state.waits_for.get(&waiter)
                != Some(&WaitEdge {
                    blocker: owner,
                    id: wait_id,
                })
            {
                return Ok(());
            }
            if state.owners.get(key) != Some(&owner) {
                clear_wait_edge(&mut state, waiter, owner, wait_id);
                return Ok(());
            }
            #[cfg(test)]
            signal_unique_wait_test_observer();
            self.changed.wait_for(&mut state, Duration::from_millis(25));
        }
    }

    pub(crate) fn claim_row_keys(
        &self,
        owner: Uuid,
        collection: &str,
        ids: &BTreeSet<String>,
    ) -> Result<RowKeyClaim, CassieError> {
        let keys = ids
            .iter()
            .cloned()
            .map(|id| RowTransactionKey {
                collection: collection.to_ascii_lowercase(),
                id,
            })
            .collect::<BTreeSet<_>>();
        let mut state = self.state.lock();
        state.waits_for.remove(&owner);
        if let Some((key, blocker)) = keys.iter().find_map(|key| {
            state
                .row_owners
                .get(key)
                .filter(|blocker| **blocker != owner)
                .map(|blocker| (key.clone(), *blocker))
        }) {
            if closes_wait_cycle(&state, owner, blocker) {
                return Err(deadlock_error());
            }
            let wait_id = Uuid::new_v4();
            state.waits_for.insert(
                owner,
                WaitEdge {
                    blocker,
                    id: wait_id,
                },
            );
            return Ok(RowKeyClaim::Wait {
                collection: key.collection,
                id: key.id,
                owner: blocker,
                wait_id,
            });
        }
        for key in &keys {
            state.row_owners.insert(key.clone(), owner);
        }
        state.rows_by_owner.entry(owner).or_default().extend(keys);
        Ok(RowKeyClaim::Claimed)
    }

    pub(crate) fn wait_for_row(
        &self,
        waiter: Uuid,
        owner: Uuid,
        collection: &str,
        id: &str,
        wait_id: Uuid,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        let key = RowTransactionKey {
            collection: collection.to_ascii_lowercase(),
            id: id.to_owned(),
        };
        let mut state = self.state.lock();
        loop {
            controls.check_interruption().inspect_err(|_| {
                clear_wait_edge(&mut state, waiter, owner, wait_id);
            })?;
            if state.waits_for.get(&waiter)
                != Some(&WaitEdge {
                    blocker: owner,
                    id: wait_id,
                })
            {
                return Ok(());
            }
            if state.row_owners.get(&key) != Some(&owner) {
                clear_wait_edge(&mut state, waiter, owner, wait_id);
                return Ok(());
            }
            #[cfg(test)]
            observe_row_wait();
            self.changed.wait_for(&mut state, Duration::from_millis(25));
        }
    }

    pub(crate) fn snapshot_owner_keys(&self, owner: Uuid) -> BTreeSet<UniqueTransactionKey> {
        self.state
            .lock()
            .keys_by_owner
            .get(&owner)
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn snapshot_owner_rows(&self, owner: Uuid) -> BTreeSet<RowTransactionKey> {
        self.state
            .lock()
            .rows_by_owner
            .get(&owner)
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn restore_owner_rows(
        &self,
        owner: Uuid,
        keys: &BTreeSet<RowTransactionKey>,
    ) -> Result<(), CassieError> {
        let mut state = self.state.lock();
        if keys.iter().any(|key| {
            state
                .row_owners
                .get(key)
                .is_some_and(|current_owner| *current_owner != owner)
        }) {
            return Err(deadlock_error());
        }
        let previous = state.rows_by_owner.remove(&owner).unwrap_or_default();
        for key in previous.difference(keys) {
            if state.row_owners.get(key) == Some(&owner) {
                state.row_owners.remove(key);
            }
        }
        for key in keys {
            state.row_owners.insert(key.clone(), owner);
        }
        if !keys.is_empty() {
            state.rows_by_owner.insert(owner, keys.clone());
        }
        self.changed.notify_all();
        Ok(())
    }

    pub(crate) fn restore_owner_keys(
        &self,
        owner: Uuid,
        keys: &BTreeSet<UniqueTransactionKey>,
    ) -> Result<(), CassieError> {
        let mut state = self.state.lock();
        if keys.iter().any(|key| {
            state
                .owners
                .get(key)
                .is_some_and(|current_owner| *current_owner != owner)
        }) {
            return Err(deadlock_error());
        }

        let previous = state.keys_by_owner.remove(&owner).unwrap_or_default();
        for key in previous.difference(keys) {
            if state.owners.get(key) == Some(&owner) {
                state.owners.remove(key);
            }
        }
        for key in keys {
            state.owners.insert(key.clone(), owner);
        }
        if !keys.is_empty() {
            state.keys_by_owner.insert(owner, keys.clone());
        }
        state.waits_for.remove(&owner);
        self.changed.notify_all();
        Ok(())
    }

    pub(crate) fn release_owner(&self, owner: Uuid) {
        let mut state = self.state.lock();
        release_owner_state(&mut state, owner);
        self.changed.notify_all();
    }
}

#[cfg(test)]
#[path = "transaction_unique_waits_issue_tests.rs"]
mod issue_tests;

fn release_owner_state(state: &mut RegistryState, owner: Uuid) {
    if let Some(keys) = state.keys_by_owner.remove(&owner) {
        for key in keys {
            if state.owners.get(&key) == Some(&owner) {
                state.owners.remove(&key);
            }
        }
    }
    if let Some(keys) = state.rows_by_owner.remove(&owner) {
        for key in keys {
            if state.row_owners.get(&key) == Some(&owner) {
                state.row_owners.remove(&key);
            }
        }
    }
    state.waits_for.remove(&owner);
}

pub(crate) fn collect_unique_keys(
    catalog: &Catalog,
    midge: &Midge,
    session: &crate::app::CassieSession,
) -> BTreeSet<UniqueTransactionKey> {
    let writes = session.transaction_writes();
    let mut keys = BTreeSet::new();
    for (collection, changes) in writes {
        let constraints = catalog.get_constraints(&collection);
        if constraints.is_empty() {
            continue;
        }
        let schema = catalog.get_schema(&collection);
        let relation = midge.canonical_collection_name(&collection);
        for constraint in &constraints {
            if !crate::catalog::enforces_single_column_uniqueness(constraint, &constraints) {
                continue;
            }
            for change in changes.values() {
                if let TransactionRowChange::Upsert(payload) = change {
                    collect_constraint_value(
                        &mut keys,
                        &relation,
                        &collection,
                        constraint,
                        schema.as_ref(),
                        payload,
                    );
                }
            }
        }
    }
    keys
}

pub(crate) fn collect_statement_unique_keys(
    catalog: &Catalog,
    midge: &Midge,
    session: &crate::app::CassieSession,
    changed_rows: &BTreeMap<String, BTreeSet<String>>,
    include_preimages: bool,
) -> Result<
    (
        BTreeSet<UniqueTransactionKey>,
        BTreeSet<UniqueTransactionKey>,
    ),
    CassieError,
> {
    let writes = session.transaction_writes();
    let mut preimages = BTreeSet::new();
    let mut candidates = BTreeSet::new();
    for (collection, ids) in changed_rows {
        let constraints = catalog.get_constraints(collection);
        if constraints.is_empty() {
            continue;
        }
        let schema = catalog.get_schema(collection);
        let relation = midge.canonical_collection_name(collection);
        let has_published_relation =
            include_preimages && midge.current_row_schema_identity(collection)?.is_some();
        let changes = writes.get(collection);
        for id in ids {
            let previous = if has_published_relation {
                midge.get_document(collection, id)?
            } else {
                None
            };
            for constraint in &constraints {
                if !crate::catalog::enforces_single_column_uniqueness(constraint, &constraints) {
                    continue;
                }
                if let Some(TransactionRowChange::Upsert(payload)) =
                    changes.and_then(|rows| rows.get(id))
                {
                    collect_constraint_value(
                        &mut candidates,
                        &relation,
                        collection,
                        constraint,
                        schema.as_ref(),
                        payload,
                    );
                }
                if let Some(previous) = &previous {
                    collect_constraint_value(
                        &mut preimages,
                        &relation,
                        collection,
                        constraint,
                        schema.as_ref(),
                        &previous.payload,
                    );
                }
            }
        }
    }
    preimages.extend(candidates.iter().cloned());
    Ok((preimages, candidates))
}

pub(crate) fn has_unique_wait_owner(
    catalog: &Catalog,
    midge: &Midge,
    session: &crate::app::CassieSession,
    collection: &str,
    field: &str,
    value: &serde_json::Value,
) -> bool {
    if !session.defers_unique_wait_duplicate() {
        return false;
    }
    let Some(waits) = session.transaction_unique_waits() else {
        return false;
    };
    let constraints = catalog.get_constraints(collection);
    let relation = midge.canonical_collection_name(collection);
    let owner = session.transaction_unique_owner_id();
    constraints.iter().any(|constraint| {
        constraint.field == field
            && crate::catalog::enforces_single_column_uniqueness(constraint, &constraints)
            && waits.has_other_owner(
                owner,
                &UniqueTransactionKey {
                    relation: relation.clone(),
                    constraint: constraint.unique_constraint_name(collection),
                    field: constraint.field.clone(),
                    value: value.to_string(),
                },
            )
    })
}

pub(crate) fn collect_upsert_unique_keys(
    catalog: &Catalog,
    midge: &Midge,
    collection: &str,
    field: &str,
    payloads: &[serde_json::Value],
) -> BTreeSet<UniqueTransactionKey> {
    let constraints = catalog.get_constraints(collection);
    let schema = catalog.get_schema(collection);
    let relation = midge.canonical_collection_name(collection);
    let mut keys = BTreeSet::new();
    for constraint in constraints.iter().filter(|constraint| {
        constraint.field == field
            && crate::catalog::enforces_single_column_uniqueness(constraint, &constraints)
    }) {
        for payload in payloads {
            let Some(value) = payload.get(field) else {
                continue;
            };
            if crate::app::field_value_is_sql_null(Some(value), schema.as_ref(), field) {
                continue;
            }
            keys.insert(UniqueTransactionKey {
                relation: relation.clone(),
                constraint: constraint.unique_constraint_name(collection),
                field: field.to_owned(),
                value: value.to_string(),
            });
        }
    }
    keys
}

pub(crate) fn validate_unique_key_candidate(
    cassie: &Cassie,
    catalog: &Catalog,
    midge: &Midge,
    session: &CassieSession,
    key: &UniqueTransactionKey,
) -> Result<(), CassieError> {
    let expected_value = crate::types::array::parse_json_document(&key.value).map_err(|error| {
        CassieError::Execution(format!("invalid staged unique-key value: {error}"))
    })?;
    for (collection, changes) in session.transaction_writes() {
        if midge.canonical_collection_name(&collection) != key.relation {
            continue;
        }
        let constraints = catalog.get_constraints(&collection);
        let Some(constraint) = constraints.iter().find(|constraint| {
            constraint.field == key.field
                && crate::catalog::enforces_single_column_uniqueness(constraint, &constraints)
                && constraint.unique_constraint_name(&collection) == key.constraint
        }) else {
            continue;
        };
        for (id, change) in changes {
            let TransactionRowChange::Upsert(payload) = change else {
                continue;
            };
            let Some(value) = payload.get(&constraint.field) else {
                continue;
            };
            if value != &expected_value {
                continue;
            }
            let Some(payload) = payload.as_object() else {
                continue;
            };
            let validation = cassie.validate_uniques(
                Some(session),
                &collection,
                payload,
                &constraints,
                Some(&id),
            );
            match validation {
                Ok(()) => {}
                Err(CassieError::UniqueViolation {
                    column, constraint, ..
                }) => {
                    let value = payload.get(&column).ok_or_else(|| {
                        CassieError::Execution(
                            "unique-key wait lost its duplicate candidate value".to_owned(),
                        )
                    })?;
                    return Err(crate::app::DiagnosticError::new(
                        "23505",
                        format!("duplicate key value violates unique constraint \"{constraint}\""),
                    )
                    .with_detail(unique_key_duplicate_detail(&column, value))
                    .into());
                }
                Err(error) => return Err(error),
            }
            return Ok(());
        }
    }
    Err(CassieError::Execution(
        "unique-key wait lost its staged INSERT candidate".to_owned(),
    ))
}

fn unique_key_duplicate_detail(field: &str, value: &serde_json::Value) -> String {
    let value = value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned);
    format!("Key ({field})=({value}) already exists.")
}

fn collect_constraint_value(
    keys: &mut BTreeSet<UniqueTransactionKey>,
    relation: &str,
    collection: &str,
    constraint: &FieldConstraint,
    schema: Option<&crate::catalog::CollectionSchema>,
    payload: &serde_json::Value,
) {
    let Some(value) = payload.get(&constraint.field) else {
        return;
    };
    if crate::app::field_value_is_sql_null(Some(value), schema, &constraint.field) {
        return;
    }
    keys.insert(UniqueTransactionKey {
        relation: relation.to_owned(),
        constraint: constraint.unique_constraint_name(collection),
        field: constraint.field.clone(),
        value: value.to_string(),
    });
}

fn closes_wait_cycle(state: &RegistryState, waiter: Uuid, blocker: Uuid) -> bool {
    let mut current = blocker;
    let mut visited = BTreeSet::new();
    while visited.insert(current) {
        if current == waiter {
            return true;
        }
        let Some(next) = state.waits_for.get(&current).map(|edge| edge.blocker) else {
            return false;
        };
        current = next;
    }
    true
}

fn clear_wait_edge(state: &mut RegistryState, waiter: Uuid, owner: Uuid, wait_id: Uuid) {
    if state.waits_for.get(&waiter)
        == Some(&WaitEdge {
            blocker: owner,
            id: wait_id,
        })
    {
        state.waits_for.remove(&waiter);
    }
}

fn deadlock_error() -> CassieError {
    CassieError::Transaction {
        kind: TransactionErrorKind::DeadlockDetected,
        message: "deadlock detected while waiting for a unique key".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CassieRuntimeLimits;
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
    use std::sync::{mpsc, Arc};
    use std::time::Instant;

    fn unique_key() -> UniqueTransactionKey {
        UniqueTransactionKey {
            relation: "db.public.items".to_owned(),
            constraint: "items_label_key".to_owned(),
            field: "label".to_owned(),
            value: "\"shared\"".to_owned(),
        }
    }

    fn waiting_pair() -> (TransactionUniqueWaits, Uuid, Uuid, UniqueTransactionKey) {
        let waits = TransactionUniqueWaits::default();
        let owner = Uuid::new_v4();
        let waiter = Uuid::new_v4();
        let key = unique_key();
        waits
            .claim_keys(owner, &BTreeSet::from([key.clone()]))
            .expect("owner claims the key");
        let claim = waits
            .claim_keys(waiter, &BTreeSet::from([key.clone()]))
            .expect("waiter requests the held key");
        assert!(matches!(claim, UniqueKeyClaim::Wait { .. }));
        (waits, owner, waiter, key)
    }

    #[test]
    fn should_clear_wait_edge_when_query_is_canceled() {
        // Arrange
        let (waits, owner, waiter, key) = waiting_pair();
        let cancellation = QueryCancellationHandle::new();
        cancellation.cancel();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation,
        );

        // Act
        let wait_id = waits
            .state
            .lock()
            .waits_for
            .get(&waiter)
            .expect("wait edge")
            .id;
        let result = waits.wait_for_key(waiter, owner, &key, wait_id, &controls);

        // Assert
        assert!(matches!(result, Err(CassieError::QueryCanceled)));
        assert!(!waits.state.lock().waits_for.contains_key(&waiter));
    }

    #[test]
    fn should_clear_wait_edge_when_query_deadline_has_elapsed() {
        // Arrange
        let (waits, owner, waiter, key) = waiting_pair();
        let limits = CassieRuntimeLimits {
            query_timeout_ms: 1,
            ..CassieRuntimeLimits::default()
        };
        let elapsed_start = Instant::now()
            .checked_sub(Duration::from_millis(10))
            .expect("subtract a short duration from the current instant");
        let controls = QueryExecutionControls::from_limits(&limits, elapsed_start);

        // Act
        let wait_id = waits
            .state
            .lock()
            .waits_for
            .get(&waiter)
            .expect("wait edge")
            .id;
        let result = waits.wait_for_key(waiter, owner, &key, wait_id, &controls);

        // Assert
        assert!(matches!(result, Err(CassieError::DeadlineExceeded)));
        assert!(!waits.state.lock().waits_for.contains_key(&waiter));
    }

    #[test]
    fn should_not_clear_newer_wait_from_synchronized_obsolete_cleanup() {
        // Arrange
        let waits = Arc::new(TransactionUniqueWaits::default());
        let blocker = Uuid::new_v4();
        let waiter = Uuid::new_v4();
        let key = unique_key();
        waits
            .claim_keys(blocker, &BTreeSet::from([key.clone()]))
            .expect("blocker claims the key");
        let UniqueKeyClaim::Wait {
            wait_id: retired_wait_id,
            ..
        } = waits
            .claim_keys(waiter, &BTreeSet::from([key.clone()]))
            .expect("retired wait claim")
        else {
            panic!("expected the original wait");
        };
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        let (waiting_sender, waiting_receiver) = mpsc::channel();
        let worker = {
            let waits = Arc::clone(&waits);
            let key = key.clone();
            std::thread::spawn(move || {
                let _observer = observe_next_unique_transaction_wait_for_test(move || {
                    waiting_sender.send(()).expect("replacement observer");
                });
                waits.wait_for_key(waiter, blocker, &key, retired_wait_id, &controls)
            })
        };
        waiting_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("obsolete waiter reaches the condition variable");
        let newer_wait_id = Uuid::new_v4();

        // Act
        {
            let mut state = waits.state.lock();
            assert_eq!(
                state.waits_for.insert(
                    waiter,
                    WaitEdge {
                        blocker,
                        id: newer_wait_id,
                    },
                ),
                Some(WaitEdge {
                    blocker,
                    id: retired_wait_id,
                })
            );
            cancellation.cancel();
            waits.changed.notify_all();
        }
        let result = worker.join().expect("obsolete cleanup worker");

        // Assert
        assert!(matches!(result, Err(CassieError::QueryCanceled)));
        assert_eq!(
            waits
                .state
                .lock()
                .waits_for
                .get(&waiter)
                .map(|edge| edge.id),
            Some(newer_wait_id)
        );
    }

    #[test]
    fn should_reject_a_unique_key_wait_cycle() {
        // Arrange
        let waits = TransactionUniqueWaits::default();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let first_key = UniqueTransactionKey {
            value: "\"first\"".to_owned(),
            ..unique_key()
        };
        let second_key = UniqueTransactionKey {
            value: "\"second\"".to_owned(),
            ..unique_key()
        };

        // Act
        let first_claim = waits
            .claim_keys(first, &BTreeSet::from([first_key.clone()]))
            .expect("first key claim");
        let second_claim = waits
            .claim_keys(second, &BTreeSet::from([second_key.clone()]))
            .expect("second key claim");
        let first_wait = waits
            .claim_keys(first, &BTreeSet::from([second_key]))
            .expect("first wait");
        let cycle = waits.claim_keys(second, &BTreeSet::from([first_key]));

        // Assert
        assert_eq!(first_claim, UniqueKeyClaim::Claimed);
        assert_eq!(second_claim, UniqueKeyClaim::Claimed);
        assert!(matches!(first_wait, UniqueKeyClaim::Wait { .. }));
        assert!(matches!(
            cycle,
            Err(CassieError::Transaction {
                kind: TransactionErrorKind::DeadlockDetected,
                ..
            })
        ));
    }

    #[test]
    fn should_release_row_owner_for_a_later_transaction() {
        // Arrange
        let waits = TransactionUniqueWaits::default();
        let owner = Uuid::new_v4();
        let waiter = Uuid::new_v4();
        let ids = BTreeSet::from(["row-1".to_owned()]);
        assert_eq!(
            waits
                .claim_row_keys(owner, "items", &ids)
                .expect("row claim"),
            RowKeyClaim::Claimed
        );

        // Act
        waits.release_owner(owner);
        let claim = waits
            .claim_row_keys(waiter, "items", &ids)
            .expect("released row claim");

        // Assert
        assert_eq!(claim, RowKeyClaim::Claimed);
    }

    #[test]
    fn should_cancel_a_row_wait_without_retaining_wait_edge() {
        // Arrange
        let waits = TransactionUniqueWaits::default();
        let owner = Uuid::new_v4();
        let waiter = Uuid::new_v4();
        let ids = BTreeSet::from(["row-1".to_owned()]);
        waits
            .claim_row_keys(owner, "items", &ids)
            .expect("row claim");
        let claim = waits
            .claim_row_keys(waiter, "items", &ids)
            .expect("row wait");
        let RowKeyClaim::Wait {
            collection,
            id,
            owner: blocker,
            wait_id,
        } = claim
        else {
            panic!("expected row wait");
        };
        let cancellation = QueryCancellationHandle::new();
        cancellation.cancel();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation,
        );

        // Act
        let result = waits.wait_for_row(waiter, blocker, &collection, &id, wait_id, &controls);

        // Assert
        assert!(matches!(result, Err(CassieError::QueryCanceled)));
        assert!(!waits.state.lock().waits_for.contains_key(&waiter));
    }
}
