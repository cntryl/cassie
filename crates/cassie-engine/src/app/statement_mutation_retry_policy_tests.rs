//! A real failed adapter proposal precedes resolver effects and native Data retry.
use super::*;
use crate::app::ProjectionCommitPolicy;
use crate::catalog::SequenceCursor;
use crate::config::CassieRuntimeConfig;
use crate::midge::adapter::data_policy_test_observer::{self, Report};
use crate::types::Value;
use cntryl_midge::{DurabilityPolicy, WriteOptions};
use std::cell::RefCell;

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Success,
    UniqueViolation,
    Other(&'static str),
}

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Initial {
        policy: DurabilityPolicy,
        outcome: Outcome,
        data: usize,
    },
    ResolverEntered {
        data: usize,
    },
    Resolved {
        policy: ProjectionCommitPolicy,
        native: SequenceCursor,
        published: SequenceCursor,
    },
    Retry {
        policy: DurabilityPolicy,
        data: usize,
    },
}

type InitialCheck = Box<dyn FnOnce(&Cassie)>;
struct Witness {
    events: Vec<Event>,
    after_unique: Option<InitialCheck>,
}
thread_local! {
    static WITNESS: RefCell<Option<Witness>> = const { RefCell::new(None) };
}
struct WitnessGuard;
impl Drop for WitnessGuard {
    fn drop(&mut self) {
        WITNESS.with(|slot| slot.borrow_mut().take());
    }
}

pub(super) fn after_initial<T>(
    cassie: &Cassie,
    options: WriteOptions,
    result: &Result<T, CassieError>,
) {
    let check = WITNESS.with(|slot| {
        let mut slot = slot.borrow_mut();
        let witness = slot.as_mut()?;
        let outcome = match result {
            Ok(_) => Outcome::Success,
            Err(CassieError::UniqueViolation { .. }) => Outcome::UniqueViolation,
            Err(error) => Outcome::Other(error.sql_state()),
        };
        let unique = outcome == Outcome::UniqueViolation;
        witness.events.push(Event::Initial {
            policy: options.policy(),
            outcome,
            data: data_policy_test_observer::current_data_submission_count().unwrap(),
        });
        if unique {
            witness.after_unique.take()
        } else {
            None
        }
    });
    if let Some(check) = check {
        check(cassie);
    }
}

pub(super) fn resolver_entered() {
    WITNESS.with(|slot| {
        if let Some(witness) = slot.borrow_mut().as_mut() {
            witness.events.push(Event::ResolverEntered {
                data: data_policy_test_observer::current_data_submission_count().unwrap(),
            });
        }
    });
}

fn sequence(cassie: &Cassie) -> crate::catalog::SequenceMeta {
    cassie
        .catalog
        .list_sequences()
        .into_iter()
        .find(|sequence| crate::catalog::local_name(&sequence.name) == "retry_witness_sequence")
        .unwrap()
}

pub(super) fn resolved(cassie: &Cassie, staging: &CassieSession) {
    WITNESS.with(|slot| {
        if let Some(witness) = slot.borrow_mut().as_mut() {
            let published = sequence(cassie);
            let native = cassie.midge.get_sequence(&published.name).unwrap().unwrap();
            witness.events.push(Event::Resolved {
                policy: staging.projection_commit_policy(false),
                native: native.cursor(),
                published: published.cursor(),
            });
        }
    });
}

pub(super) fn before_retry(options: WriteOptions) {
    WITNESS.with(|slot| {
        if let Some(witness) = slot.borrow_mut().as_mut() {
            witness.events.push(Event::Retry {
                policy: options.policy(),
                data: data_policy_test_observer::current_data_submission_count().unwrap(),
            });
        }
    });
}

struct Fixture {
    cassie: Cassie,
    session: CassieSession,
    other: CassieSession,
    cursor: SequenceCursor,
}

fn fixture(sequence_effect: bool, competitor: bool) -> Fixture {
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("retry_witness", None);
    let other = cassie.create_session("competitor", None);
    for sql in [
        "CREATE TABLE retry_witness_rows (email TEXT UNIQUE, value BIGINT)",
        "CREATE TABLE retry_witness_checkpoint (position INT)",
        "CREATE SEQUENCE retry_witness_sequence",
        "BEGIN",
        "SET LOCAL cassie.projection_write_mode = 'replayable'",
    ] {
        cassie.execute_sql(&session, sql, vec![]).unwrap();
    }
    let assignment = if sequence_effect {
        "nextval('retry_witness_sequence')"
    } else {
        "excluded.value"
    };
    cassie.execute_sql(&session, &format!("INSERT INTO retry_witness_rows (email, value) VALUES ('a', 1) ON CONFLICT (email) DO UPDATE SET value = {assignment}"), vec![]).unwrap();
    cassie
        .execute_sql(
            &session,
            "INSERT INTO retry_witness_checkpoint VALUES (7)",
            vec![],
        )
        .unwrap();
    if competitor {
        cassie
            .execute_sql(
                &other,
                "INSERT INTO retry_witness_rows (email, value) VALUES ('a', 9)",
                vec![],
            )
            .unwrap();
    }
    let cursor = sequence(&cassie).cursor();
    Fixture {
        cassie,
        session,
        other,
        cursor,
    }
}

fn commit(
    fixture: &Fixture,
    unknown_after_success: bool,
) -> (
    Result<crate::executor::QueryResult, CassieError>,
    Report,
    Vec<Event>,
) {
    let other = fixture.other.clone();
    let prior = fixture.cursor;
    WITNESS.with(|slot| {
        assert!(slot
            .replace(Some(Witness {
                events: vec![],
                after_unique: Some(Box::new(move |cassie| {
                    assert_eq!(
                        cassie
                            .execute_sql(&other, "SELECT value FROM retry_witness_rows", vec![])
                            .unwrap()
                            .rows(),
                        &[vec![Value::Int64(9)]]
                    );
                    assert_eq!(
                        cassie
                            .execute_sql(
                                &other,
                                "SELECT position FROM retry_witness_checkpoint",
                                vec![]
                            )
                            .unwrap()
                            .rows(),
                        &[] as &[Vec<Value>]
                    );
                    let published = sequence(cassie);
                    assert_eq!(published.cursor(), prior);
                    assert_eq!(
                        cassie
                            .midge
                            .get_sequence(&published.name)
                            .unwrap()
                            .unwrap()
                            .cursor(),
                        prior
                    );
                }))
            }))
            .is_none());
    });
    let _guard = WitnessGuard;
    let observation = data_policy_test_observer::arm(&fixture.cassie, "retry_policy_unit");
    let result =
        data_policy_test_observer::observe_worker(&fixture.cassie, "retry_policy_unit", || {
            if unknown_after_success {
                data_policy_test_observer::arm_existing_unknown_on_next_data_submission();
            }
            fixture
                .cassie
                .execute_sql(&fixture.session, "COMMIT", vec![])
        });
    let report = observation.report.blocking_recv().unwrap();
    let events = WITNESS.with(|slot| slot.borrow_mut().take().unwrap().events);
    (result, report, events)
}

fn assert_successful_retry(sequence_effect: bool) {
    // Arrange
    let fixture = fixture(sequence_effect, true);

    // Act
    let (result, report, events) = commit(&fixture, false);
    let rows = fixture
        .cassie
        .execute_sql(
            &fixture.other,
            "SELECT value FROM retry_witness_rows",
            vec![],
        )
        .unwrap();
    let checkpoint = fixture
        .cassie
        .execute_sql(
            &fixture.other,
            "SELECT position FROM retry_witness_checkpoint",
            vec![],
        )
        .unwrap();
    let expected_cursor = if sequence_effect {
        SequenceCursor {
            last_value: 1,
            is_called: true,
        }
    } else {
        fixture.cursor
    };
    let expected_policy = if sequence_effect {
        DurabilityPolicy::Sync
    } else {
        DurabilityPolicy::Buffered
    };

    // Assert
    result.unwrap();
    assert_eq!(
        events,
        vec![
            Event::Initial {
                policy: DurabilityPolicy::Buffered,
                outcome: Outcome::UniqueViolation,
                data: 0
            },
            Event::ResolverEntered { data: 0 },
            Event::Resolved {
                policy: if sequence_effect {
                    ProjectionCommitPolicy::Strict
                } else {
                    ProjectionCommitPolicy::Replayable
                },
                native: expected_cursor,
                published: expected_cursor
            },
            Event::Retry {
                policy: expected_policy,
                data: 0
            },
        ]
    );
    assert_eq!(
        report.data.len(),
        1,
        "actual native Data attempts: {report:?}"
    );
    assert_eq!(
        report.data[0].policy,
        if sequence_effect {
            "local_sync"
        } else {
            "local_buffered"
        }
    );
    assert!(report.data[0].succeeded);
    assert!(!report.data[0].outcome_unknown);
    assert_eq!(report.native.failed, 0);
    assert_eq!(report.native.outcome_unknown, 0);
    assert_eq!(rows.rows(), &[vec![Value::Int64(1)]]);
    assert_eq!(checkpoint.rows(), &[vec![Value::Int64(7)]]);
    let published = sequence(&fixture.cassie);
    assert_eq!(published.cursor(), expected_cursor);
    assert_eq!(
        fixture
            .cassie
            .midge
            .get_sequence(&published.name)
            .unwrap()
            .unwrap()
            .cursor(),
        expected_cursor
    );
    if sequence_effect {
        assert_eq!(
            fixture
                .session
                .current_sequence_value(
                    &published.name,
                    published.object_id,
                    "retry_witness_sequence"
                )
                .unwrap(),
            1
        );
    }
}

#[test]
fn should_promote_a_real_unique_failure_resolver_sequence_to_a_strict_data_retry() {
    // Arrange / Act / Assert: the helper executes the real losing-upsert branch and exact phase oracle.
    assert_successful_retry(true);
}

#[test]
fn should_keep_a_real_unique_failure_without_an_independent_effect_buffered_on_retry() {
    // Arrange / Act / Assert: the matched branch proves zero initial Data submissions and one buffered retry.
    assert_successful_retry(false);
}

#[test]
fn should_fence_an_unknown_response_after_native_data_success_without_resolving_or_retrying() {
    // Arrange
    let fixture = fixture(false, false);

    // Act
    let (result, report, events) = commit(&fixture, true);
    let read = fixture.cassie.execute_sql(
        &fixture.other,
        "SELECT value FROM retry_witness_rows",
        vec![],
    );
    let begin = fixture
        .cassie
        .execute_sql(&fixture.session, "BEGIN", vec![]);

    // Assert
    assert_eq!(result.unwrap_err().sql_state(), "58030");
    assert_eq!(
        events,
        vec![Event::Initial {
            policy: DurabilityPolicy::Buffered,
            outcome: Outcome::Other("58030"),
            data: 1
        }]
    );
    assert_eq!(report.data.len(), 1);
    assert_eq!(report.data[0].policy, "local_buffered");
    assert!(!report.data[0].succeeded);
    assert!(
        report.data[0].outcome_unknown,
        "adapter response simulation after actual native success"
    );
    assert!(report.native.succeeded > 0);
    assert_eq!(report.native.failed, 0);
    assert_eq!(
        report.native.outcome_unknown, 0,
        "native success precedes the simulated unknown response"
    );
    assert_eq!(read.unwrap_err().sql_state(), "58030");
    assert_eq!(begin.unwrap_err().sql_state(), "58030");
    assert!(fixture.cassie.check_storage_recovery_required().is_err());
}
