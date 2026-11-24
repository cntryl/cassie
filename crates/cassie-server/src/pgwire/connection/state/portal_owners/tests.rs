//! Portal lifetime and retained-metadata admission, independent of operation epochs.

use super::*;
use crate::config::CassieRuntimeLimits;
use std::time::Instant;
use uuid::Uuid;

fn identity(
    transaction: Option<Uuid>,
    generation: u64,
    rollback_epoch: u64,
) -> FailureAuthorityIdentity {
    FailureAuthorityIdentity {
        transaction,
        generation,
        rollback_epoch,
    }
}

fn controls(bytes: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: bytes,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

#[test]
fn should_keep_transaction_portals_across_a_savepoint_epoch_change() {
    // Arrange
    let controls = controls(4096);
    let owner = identity(Some(Uuid::new_v4()), 7, 0);
    let mut owners = PortalOwners::default();
    let pending = owners
        .prepare("p", owner, &controls)
        .expect("owner admission");
    owners.publish(pending);
    // Act
    let current = identity(owner.transaction, 7, 1);
    // Assert
    assert!(owners.belongs_to("p", current));
    assert!(!owners.belongs_to("p", identity(Some(Uuid::new_v4()), 7, 1)));
}

#[test]
fn should_adopt_only_the_recorded_idle_preparation_segment() {
    // Arrange
    let controls = controls(4096);
    let before = identity(None, 2, 0);
    let after = identity(Some(Uuid::new_v4()), 3, 0);
    let other = identity(None, 1, 0);
    let mut owners = PortalOwners::default();
    for (name, owner) in [("own", before), ("other", other), ("current", after)] {
        let pending = owners
            .prepare(name, owner, &controls)
            .expect("owner admission");
        owners.publish(pending);
    }
    // Act
    owners.adopt(before, after);
    // Assert
    assert!(owners.belongs_to("own", after));
    assert!(owners.belongs_to("other", other));
    assert!(owners.belongs_to("current", after));
    assert!(!owners.belongs_to("other", after));
}

#[test]
fn should_release_the_complete_owner_metadata_charge_when_the_last_portal_closes() {
    // Arrange
    let bytes = 4 * std::mem::size_of::<PortalOwner>() + "portal".len();
    let denied = controls(bytes - 1);
    let exact = controls(bytes);
    let owner = identity(None, 0, 0);
    let mut rejected = PortalOwners::default();
    let mut owners = PortalOwners::default();
    // Act
    let failure = rejected.prepare("portal", owner, &denied);
    let pending = owners
        .prepare("portal", owner, &exact)
        .expect("exact admission");
    owners.publish(pending);
    let retained = exact.current_query_memory_bytes();
    owners.remove("portal");
    // Assert
    assert!(matches!(failure, Err(CassieError::ResourceLimit(_))));
    assert_eq!(denied.current_query_memory_bytes(), 0);
    assert_eq!(retained, bytes);
    assert_eq!(exact.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_the_previous_owner_when_a_replacement_cannot_be_admitted() {
    // Arrange
    let controls = controls(4096);
    let before = identity(Some(Uuid::new_v4()), 1, 0);
    let after = identity(Some(Uuid::new_v4()), 2, 0);
    let mut owners = PortalOwners::default();
    let pending = owners
        .prepare("p", before, &controls)
        .expect("initial owner");
    owners.publish(pending);
    let baseline = controls.current_query_memory_bytes();
    let remaining = controls
        .reserve_query_memory(4096 - baseline)
        .expect("occupy budget");
    // Act
    let failure = owners.prepare("p", after, &controls);
    drop(remaining);
    // Assert
    assert!(matches!(failure, Err(CassieError::ResourceLimit(_))));
    assert!(owners.belongs_to("p", before));
    assert_eq!(controls.current_query_memory_bytes(), baseline);
}
