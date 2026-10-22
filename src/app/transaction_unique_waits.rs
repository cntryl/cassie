//! Transaction scoped ownership for single column unique keys staged by INSERT.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use uuid::Uuid;

use crate::app::{Cassie, CassieError, CassieSession, TransactionErrorKind, TransactionRowChange};
use crate::catalog::{Catalog, FieldConstraint};
use crate::midge::adapter::Midge;
use crate::runtime::QueryExecutionControls;

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
    },
}

#[derive(Debug, Default)]
struct RegistryState {
    owners: BTreeMap<UniqueTransactionKey, Uuid>,
    keys_by_owner: BTreeMap<Uuid, BTreeSet<UniqueTransactionKey>>,
    waits_for: BTreeMap<Uuid, Uuid>,
}

#[derive(Debug, Default)]
pub(crate) struct TransactionUniqueWaits {
    state: Mutex<RegistryState>,
    changed: Condvar,
}

impl TransactionUniqueWaits {
    pub(crate) fn claim_keys(
        &self,
        owner: Uuid,
        keys: &BTreeSet<UniqueTransactionKey>,
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
            if closes_wait_cycle(&state, owner, blocker) {
                return Err(deadlock_error());
            }
            state.waits_for.insert(owner, blocker);
            return Ok(UniqueKeyClaim::Wait {
                key,
                owner: blocker,
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
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        let mut state = self.state.lock();
        loop {
            if let Err(error) = controls.check_interruption() {
                clear_wait_edge(&mut state, waiter, owner);
                return Err(error);
            }
            if state.owners.get(key) != Some(&owner) {
                clear_wait_edge(&mut state, waiter, owner);
                return Ok(());
            }
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
        self.changed.notify_all();
        Ok(())
    }

    pub(crate) fn release_owner(&self, owner: Uuid) {
        let mut state = self.state.lock();
        release_owner_state(&mut state, owner);
        self.changed.notify_all();
    }
}

fn release_owner_state(state: &mut RegistryState, owner: Uuid) {
    if let Some(keys) = state.keys_by_owner.remove(&owner) {
        for key in keys {
            if state.owners.get(&key) == Some(&owner) {
                state.owners.remove(&key);
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
            collect_constraint_values(
                &mut keys,
                &relation,
                &collection,
                constraint,
                schema.as_ref(),
                &changes,
            );
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
    let expected_value: serde_json::Value = serde_json::from_str(&key.value).map_err(|error| {
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
            cassie.validate_uniques(
                Some(session),
                &collection,
                payload,
                &constraints,
                Some(&id),
            )?;
            return Ok(());
        }
    }
    Err(CassieError::Execution(
        "unique-key wait lost its staged INSERT candidate".to_owned(),
    ))
}

fn collect_constraint_values(
    keys: &mut BTreeSet<UniqueTransactionKey>,
    relation: &str,
    collection: &str,
    constraint: &FieldConstraint,
    schema: Option<&crate::catalog::CollectionSchema>,
    changes: &BTreeMap<String, TransactionRowChange>,
) {
    for change in changes.values() {
        let TransactionRowChange::Upsert(payload) = change else {
            continue;
        };
        let Some(value) = payload.get(&constraint.field) else {
            continue;
        };
        if crate::app::field_value_is_sql_null(Some(value), schema, &constraint.field) {
            continue;
        }
        keys.insert(UniqueTransactionKey {
            relation: relation.to_owned(),
            constraint: constraint.unique_constraint_name(collection),
            field: constraint.field.clone(),
            value: value.to_string(),
        });
    }
}

fn closes_wait_cycle(state: &RegistryState, waiter: Uuid, blocker: Uuid) -> bool {
    let mut current = blocker;
    let mut visited = BTreeSet::new();
    while visited.insert(current) {
        if current == waiter {
            return true;
        }
        let Some(next) = state.waits_for.get(&current) else {
            return false;
        };
        current = *next;
    }
    true
}

fn clear_wait_edge(state: &mut RegistryState, waiter: Uuid, owner: Uuid) {
    if state.waits_for.get(&waiter) == Some(&owner) {
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
        assert!(matches!(
            waits
                .claim_keys(waiter, &BTreeSet::from([key.clone()]))
                .expect("waiter requests the held key"),
            UniqueKeyClaim::Wait { .. }
        ));
        (waits, owner, waiter, key)
    }

    #[test]
    fn should_clear_wait_edge_when_query_is_cancelled() {
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
        let result = waits.wait_for_key(waiter, owner, &key, &controls);

        // Assert
        assert!(matches!(result, Err(CassieError::QueryCancelled)));
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
        let result = waits.wait_for_key(waiter, owner, &key, &controls);

        // Assert
        assert!(matches!(result, Err(CassieError::DeadlineExceeded)));
        assert!(!waits.state.lock().waits_for.contains_key(&waiter));
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
}
