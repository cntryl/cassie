use std::cell::RefCell;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::rc::Rc;

use crate::app::{Cassie, CassieError, CassieSession};
use crate::catalog::ProjectionVersionState;
use crate::config::CassieRuntimeConfig;
use crate::runtime::QueryCancellationHandle;
use crate::types::Value;

thread_local! {
    static CANCEL_BEFORE_ACTIVATION: RefCell<Option<QueryCancellationHandle>> = const {
        RefCell::new(None)
    };
}

struct CancelBeforeActivationGuard {
    _thread_bound: PhantomData<Rc<()>>,
}

impl Drop for CancelBeforeActivationGuard {
    fn drop(&mut self) {
        CANCEL_BEFORE_ACTIVATION.with(|pending| {
            pending.borrow_mut().take();
        });
    }
}

struct ActivationFailureGuard;

impl Drop for ActivationFailureGuard {
    fn drop(&mut self) {
        crate::executor::set_projection_activation_failure_point(false);
    }
}

struct Directory(PathBuf);

impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("isolated materialized-view storage cleanup");
    }
}

fn create_materialized_view_with_unique_index(cassie: &Cassie) -> (CassieSession, String) {
    let session = cassie.create_session("tester", None);
    for statement in [
        "CREATE TABLE concurrent_refresh_cancel_source (id INT PRIMARY KEY, label TEXT)",
        "INSERT INTO concurrent_refresh_cancel_source VALUES (1, 'before')",
        "CREATE MATERIALIZED VIEW concurrent_refresh_cancel_view AS \
           SELECT id, label FROM concurrent_refresh_cancel_source WITH DATA",
        "CREATE UNIQUE INDEX concurrent_refresh_cancel_view_id_idx \
           ON concurrent_refresh_cancel_view (id)",
    ] {
        cassie
            .execute_sql(&session, statement, vec![])
            .unwrap_or_else(|error| panic!("execute {statement}: {error}"));
    }
    let initial = cassie
        .catalog
        .get_materialized_projection("concurrent_refresh_cancel_view")
        .expect("materialized-view metadata");
    let active_version = initial
        .active_version
        .expect("initial output version is active");
    assert_eq!(
        cassie
            .catalog
            .list_indexes("concurrent_refresh_cancel_view")
            .len(),
        1
    );
    (session, active_version)
}

fn materialized_view_with_unique_index() -> (Cassie, CassieSession, String) {
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
        .expect("create in-memory Cassie");
    cassie.startup().expect("start Cassie");
    let (session, active_version) = create_materialized_view_with_unique_index(&cassie);
    (cassie, session, active_version)
}

fn cancel_next_refresh_before_activation(
    cancellation: QueryCancellationHandle,
) -> CancelBeforeActivationGuard {
    CANCEL_BEFORE_ACTIVATION.with(|pending| {
        assert!(
            pending.borrow_mut().replace(cancellation).is_none(),
            "concurrent refresh cancellation hooks must not be nested"
        );
    });
    CancelBeforeActivationGuard {
        _thread_bound: PhantomData,
    }
}

pub(super) fn cancel_before_activation_if_armed() {
    CANCEL_BEFORE_ACTIVATION.with(|pending| {
        if let Some(cancellation) = pending.borrow_mut().take() {
            cancellation.cancel();
        }
    });
}

#[test]
fn should_cleanup_unpublished_indexes_when_concurrent_refresh_is_cancelled_before_activation() {
    // Arrange
    let (cassie, session, previous_version) = materialized_view_with_unique_index();
    cassie
        .execute_sql(
            &session,
            "UPDATE concurrent_refresh_cancel_source SET label = 'after' WHERE id = 1",
            vec![],
        )
        .expect("change source row before refresh");
    let previous_indexes = cassie
        .catalog
        .list_indexes("concurrent_refresh_cancel_view")
        .into_iter()
        .map(|index| index.name)
        .collect::<Vec<_>>();
    let cancellation = QueryCancellationHandle::new();
    let _cancel_at_activation = cancel_next_refresh_before_activation(cancellation.clone());

    // Act
    let error = cassie
        .execute_sql_with_cancellation(
            &session,
            "REFRESH MATERIALIZED VIEW CONCURRENTLY concurrent_refresh_cancel_view",
            vec![],
            &cancellation,
        )
        .expect_err("cancellation before activation should abort the refresh");
    let after = cassie
        .catalog
        .get_materialized_projection("concurrent_refresh_cancel_view")
        .expect("materialized-view metadata after cancellation");
    let failed_version = after
        .versions
        .iter()
        .find(|version| version.version_id != previous_version)
        .expect("failed refresh version remains recorded for diagnostics");
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT id, label FROM concurrent_refresh_cancel_view ORDER BY id",
            vec![],
        )
        .expect("read the previously active output")
        .rows()
        .to_vec();

    // Assert
    assert!(matches!(error, CassieError::QueryCancelled));
    assert_eq!(
        after.active_version.as_deref(),
        Some(previous_version.as_str())
    );
    assert_eq!(failed_version.state, ProjectionVersionState::Failed);
    assert!(
        cassie
            .midge
            .collection_schema(&failed_version.output_collection)
            .is_none(),
        "cancelled refresh output must be discarded"
    );
    assert!(cassie
        .catalog
        .all_indexes_snapshot()
        .iter()
        .all(|index| index.collection != failed_version.output_collection));
    assert_eq!(
        cassie
            .catalog
            .list_indexes("concurrent_refresh_cancel_view")
            .into_iter()
            .map(|index| index.name)
            .collect::<Vec<_>>(),
        previous_indexes
    );
    assert_eq!(
        rows,
        vec![vec![Value::Int64(1), Value::String("before".to_owned())]]
    );

    cassie
        .execute_sql(
            &session,
            "REFRESH MATERIALIZED VIEW CONCURRENTLY concurrent_refresh_cancel_view",
            vec![],
        )
        .expect("a later refresh should publish successfully");
    let refreshed_rows = cassie
        .execute_sql(
            &session,
            "SELECT id, label FROM concurrent_refresh_cancel_view ORDER BY id",
            vec![],
        )
        .expect("read the successfully refreshed output")
        .rows()
        .to_vec();
    assert_eq!(
        cassie
            .catalog
            .list_indexes("concurrent_refresh_cancel_view")
            .len(),
        1
    );
    assert_eq!(
        refreshed_rows,
        vec![vec![Value::Int64(1), Value::String("after".to_owned())]]
    );
}

#[test]
fn should_cleanup_unpublished_output_given_activation_fails_before_publication() {
    // Arrange
    let (cassie, session, previous_version) = materialized_view_with_unique_index();
    cassie
        .execute_sql(
            &session,
            "UPDATE concurrent_refresh_cancel_source SET label = 'after' WHERE id = 1",
            vec![],
        )
        .expect("change source row before refresh");
    crate::executor::set_projection_activation_failure_point(true);
    let _reset_activation_failure_point = ActivationFailureGuard;

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "REFRESH MATERIALIZED VIEW CONCURRENTLY concurrent_refresh_cancel_view",
            vec![],
        )
        .expect_err("activation failure should abort the refresh");
    let after = cassie
        .catalog
        .get_materialized_projection("concurrent_refresh_cancel_view")
        .expect("materialized-view metadata after activation failure");
    let failed_version = after
        .versions
        .iter()
        .find(|version| version.version_id != previous_version)
        .expect("failed refresh version remains recorded for diagnostics");
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT id, label FROM concurrent_refresh_cancel_view ORDER BY id",
            vec![],
        )
        .expect("read the previously active output")
        .rows()
        .to_vec();

    // Assert
    assert!(error
        .to_string()
        .contains("injected projection failure before activation publication"));
    assert_eq!(
        after.active_version.as_deref(),
        Some(previous_version.as_str())
    );
    assert_eq!(failed_version.state, ProjectionVersionState::Failed);
    assert!(cassie
        .midge
        .collection_schema(&failed_version.output_collection)
        .is_none());
    assert!(cassie
        .catalog
        .all_indexes_snapshot()
        .iter()
        .all(|index| index.collection != failed_version.output_collection));
    assert_eq!(
        cassie
            .catalog
            .list_indexes("concurrent_refresh_cancel_view")
            .len(),
        1
    );
    assert_eq!(
        rows,
        vec![vec![Value::Int64(1), Value::String("before".to_owned())]]
    );

    cassie
        .execute_sql(
            &session,
            "REFRESH MATERIALIZED VIEW CONCURRENTLY concurrent_refresh_cancel_view",
            vec![],
        )
        .expect("a later refresh should publish successfully");
    let refreshed_rows = cassie
        .execute_sql(
            &session,
            "SELECT id, label FROM concurrent_refresh_cancel_view ORDER BY id",
            vec![],
        )
        .expect("read the successfully refreshed output")
        .rows()
        .to_vec();
    assert_eq!(
        refreshed_rows,
        vec![vec![Value::Int64(1), Value::String("after".to_owned())]]
    );
}

fn cancel_refresh_before_activation(
    cassie: &Cassie,
    session: &CassieSession,
    previous_version: &str,
) -> (String, String) {
    let cancellation = QueryCancellationHandle::new();
    let _cancel_at_activation = cancel_next_refresh_before_activation(cancellation.clone());
    let error = cassie
        .execute_sql_with_cancellation(
            session,
            "REFRESH MATERIALIZED VIEW CONCURRENTLY concurrent_refresh_cancel_view",
            vec![],
            &cancellation,
        )
        .expect_err("cancellation before activation should abort the refresh");
    assert!(matches!(error, CassieError::QueryCancelled));
    failed_candidate_identity(cassie, previous_version)
}

fn failed_candidate_identity(cassie: &Cassie, previous_version: &str) -> (String, String) {
    let metadata = cassie
        .catalog
        .get_materialized_projection("concurrent_refresh_cancel_view")
        .expect("materialized-view metadata after cancellation");
    assert_eq!(metadata.active_version.as_deref(), Some(previous_version));
    let failed_version = metadata
        .versions
        .iter()
        .find(|version| version.version_id != previous_version)
        .expect("failed refresh version remains recorded");
    assert_eq!(failed_version.state, ProjectionVersionState::Failed);
    (
        failed_version.version_id.clone(),
        failed_version.output_collection.clone(),
    )
}

fn materialized_view_rows(cassie: &Cassie, session: &CassieSession) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(
            session,
            "SELECT id, label FROM concurrent_refresh_cancel_view ORDER BY id",
            vec![],
        )
        .expect("read materialized-view rows")
        .rows()
        .to_vec()
}

fn assert_failed_refresh_cleanup(
    cassie: &Cassie,
    previous_version: &str,
    failed_version_id: &str,
    failed_output_collection: &str,
) {
    let (failed_id, failed_output) = failed_candidate_identity(cassie, previous_version);
    assert_eq!(failed_id, failed_version_id);
    assert_eq!(failed_output, failed_output_collection);
    assert!(cassie
        .midge
        .collection_schema(failed_output_collection)
        .is_none());
    assert!(cassie
        .catalog
        .all_indexes_snapshot()
        .iter()
        .all(|index| index.collection != failed_output_collection));
    assert_eq!(
        cassie
            .catalog
            .list_indexes("concurrent_refresh_cancel_view")
            .len(),
        1
    );
}

#[test]
fn should_recover_previous_output_after_cancelled_concurrent_refresh_restart() {
    // Arrange
    let directory = Directory(std::env::temp_dir().join(format!(
        "cassie-826-cancel-refresh-{}",
        uuid::Uuid::new_v4()
    )));
    std::fs::create_dir_all(&directory.0).expect("create isolated local store");
    let cassie = Cassie::new_with_data_dir_and_config(&directory.0, CassieRuntimeConfig::default())
        .expect("create local Cassie");
    cassie.startup().expect("start Cassie");
    let (session, previous_version) = create_materialized_view_with_unique_index(&cassie);
    cassie
        .execute_sql(
            &session,
            "UPDATE concurrent_refresh_cancel_source SET label = 'after' WHERE id = 1",
            vec![],
        )
        .expect("change source row before refresh");

    // Act
    let (failed_version_id, failed_output_collection) =
        cancel_refresh_before_activation(&cassie, &session, &previous_version);
    assert_failed_refresh_cleanup(
        &cassie,
        &previous_version,
        &failed_version_id,
        &failed_output_collection,
    );
    assert_eq!(
        materialized_view_rows(&cassie, &session),
        vec![vec![Value::Int64(1), Value::String("before".to_owned())]]
    );
    drop(session);
    drop(cassie);

    // Assert
    let restarted =
        Cassie::new_with_data_dir_and_config(&directory.0, CassieRuntimeConfig::default())
            .expect("reopen local Cassie");
    restarted.startup().expect("restart Cassie");
    let restarted_session = restarted.create_session("tester", None);
    assert_failed_refresh_cleanup(
        &restarted,
        &previous_version,
        &failed_version_id,
        &failed_output_collection,
    );
    assert_eq!(
        materialized_view_rows(&restarted, &restarted_session),
        vec![vec![Value::Int64(1), Value::String("before".to_owned())]]
    );

    restarted
        .execute_sql(
            &restarted_session,
            "REFRESH MATERIALIZED VIEW CONCURRENTLY concurrent_refresh_cancel_view",
            vec![],
        )
        .expect("a later refresh should publish successfully after restart");
    assert_eq!(
        materialized_view_rows(&restarted, &restarted_session),
        vec![vec![Value::Int64(1), Value::String("after".to_owned())]]
    );
}

#[test]
fn should_preserve_active_output_given_cancellation_before_activation_publication() {
    // Arrange
    let (cassie, session, previous_version) = materialized_view_with_unique_index();
    cassie
        .execute_sql(
            &session,
            "UPDATE concurrent_refresh_cancel_source SET label = 'after' WHERE id = 1",
            vec![],
        )
        .expect("change source row before refresh");
    let cancellation = QueryCancellationHandle::new();
    let _cancel_before_publication =
        super::super::materialized_projection::test_support::cancel_before_activation_publication(
            cancellation.clone(),
        );

    // Act
    let error = cassie
        .execute_sql_with_cancellation(
            &session,
            "REFRESH MATERIALIZED VIEW CONCURRENTLY concurrent_refresh_cancel_view",
            vec![],
            &cancellation,
        )
        .expect_err("cancellation before activation publication should abort the refresh");

    // Assert
    assert!(matches!(error, CassieError::QueryCancelled));
    let (failed_version_id, failed_output_collection) =
        failed_candidate_identity(&cassie, &previous_version);
    assert_failed_refresh_cleanup(
        &cassie,
        &previous_version,
        &failed_version_id,
        &failed_output_collection,
    );
    assert_eq!(
        materialized_view_rows(&cassie, &session),
        vec![vec![Value::Int64(1), Value::String("before".to_owned())]]
    );

    cassie
        .execute_sql(
            &session,
            "REFRESH MATERIALIZED VIEW CONCURRENTLY concurrent_refresh_cancel_view",
            vec![],
        )
        .expect("a later refresh should publish successfully");
    assert_eq!(
        materialized_view_rows(&cassie, &session),
        vec![vec![Value::Int64(1), Value::String("after".to_owned())]]
    );
}

#[test]
fn should_complete_refresh_given_cancellation_after_final_activation_check() {
    // Arrange
    let (cassie, session, previous_version) = materialized_view_with_unique_index();
    cassie
        .execute_sql(
            &session,
            "UPDATE concurrent_refresh_cancel_source SET label = 'after' WHERE id = 1",
            vec![],
        )
        .expect("change source row before refresh");
    let cancellation = QueryCancellationHandle::new();
    let _cancel_after_final_check =
        super::super::materialized_projection::test_support::
            cancel_after_activation_interruption_check(cancellation.clone());

    // Act
    let result = cassie
        .execute_sql_with_cancellation(
            &session,
            "REFRESH MATERIALIZED VIEW CONCURRENTLY concurrent_refresh_cancel_view",
            vec![],
            &cancellation,
        )
        .expect("activation already passed its final interruption check");
    let metadata = cassie
        .catalog
        .get_materialized_projection("concurrent_refresh_cancel_view")
        .expect("materialized-view metadata after refresh");
    let active_version = metadata
        .versions
        .iter()
        .find(|version| version.version_id != previous_version)
        .expect("replacement version remains recorded");

    // Assert
    assert!(cancellation.is_cancelled());
    assert_eq!(result.command(), "REFRESH MATERIALIZED VIEW");
    assert_eq!(
        metadata.active_version.as_deref(),
        Some(active_version.version_id.as_str())
    );
    assert_eq!(active_version.state, ProjectionVersionState::Active);
    assert_eq!(
        materialized_view_rows(&cassie, &session),
        vec![vec![Value::Int64(1), Value::String("after".to_owned())]]
    );
    assert_eq!(
        cassie
            .catalog
            .list_indexes("concurrent_refresh_cancel_view")
            .len(),
        1
    );
}
