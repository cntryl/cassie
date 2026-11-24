#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    use super::super::{
        query_scan_control_test_guard, set_query_scan_cancellation_after_entries,
        should_cancel_controlled_query_scan,
    };

    #[test]
    fn should_isolate_query_scan_controls_by_test_thread() {
        // Arrange
        let unrelated_guard = query_scan_control_test_guard();
        let armed = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let worker_armed = Arc::clone(&armed);
        let worker_release = Arc::clone(&release);
        let worker = std::thread::spawn(move || {
            let _controlled_guard = query_scan_control_test_guard();
            let worker_controls = controls();
            worker_armed.wait();
            worker_release.wait();
            set_query_scan_cancellation_after_entries(Some(2));
            let controlled = (hook(&worker_controls), hook(&worker_controls));
            set_query_scan_cancellation_after_entries(None);
            controlled
        });
        armed.wait();

        // Act
        let unrelated = hook(&controls());
        drop(unrelated_guard);
        release.wait();
        let controlled = worker.join().expect("query scan control worker");

        // Assert
        assert!(!unrelated);
        assert_eq!(controlled, (false, true));
    }

    fn controls() -> crate::runtime::QueryExecutionControls {
        crate::runtime::QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            std::time::Instant::now(),
        )
    }

    fn hook(controls: &crate::runtime::QueryExecutionControls) -> bool {
        should_cancel_controlled_query_scan(&controls.storage_request())
    }

    #[test]
    fn should_preserve_the_owned_scan_cancellation_boundary() {
        // Arrange
        let _guard = query_scan_control_test_guard();
        let owned = controls();
        let worker_controls = owned.clone();
        set_query_scan_cancellation_after_entries(Some(2));
        let ready = Arc::new(Barrier::new(2));
        let worker_ready = Arc::clone(&ready);
        let unrelated = std::thread::spawn(move || {
            let unrelated = controls();
            worker_ready.wait();
            hook(&unrelated)
        });
        ready.wait();

        // Act
        let unrelated = unrelated.join().expect("unrelated scanner");
        let owned = std::thread::spawn(move || (hook(&worker_controls), hook(&worker_controls)))
            .join()
            .expect("owned blocking scanner");
        set_query_scan_cancellation_after_entries(None);

        // Assert
        assert!(!unrelated);
        assert_eq!(owned, (false, true));
    }

    #[test]
    fn should_keep_late_workers_outside_a_new_fixture_scope() {
        // Arrange
        let first_guard = query_scan_control_test_guard();
        let old_controls = controls();
        drop(first_guard);
        let _new_guard = query_scan_control_test_guard();
        let new_controls = controls();
        set_query_scan_cancellation_after_entries(Some(2));

        // Act
        let old = std::thread::spawn(move || (hook(&old_controls), hook(&old_controls)))
            .join()
            .expect("late old scanner");
        let new = (hook(&new_controls), hook(&new_controls));
        set_query_scan_cancellation_after_entries(None);

        // Assert
        assert_eq!(old, (false, false));
        assert_eq!(new, (false, true));
    }

    #[test]
    fn should_preserve_scope_through_cancellation_control_variants() {
        // Arrange
        let _guard = query_scan_control_test_guard();
        let cancellation = crate::runtime::QueryCancellationHandle::default();
        let controlled = crate::runtime::QueryExecutionControls::with_cancellation(
            &crate::config::CassieRuntimeLimits::default(),
            std::time::Instant::now(),
            cancellation.clone(),
        );
        let cloned = controlled.clone();
        let uncapped = controlled.for_uncapped_input();
        set_query_scan_cancellation_after_entries(Some(3));

        // Act
        let before = hook(&controlled);
        let worker = std::thread::spawn(move || (hook(&cloned), hook(&uncapped)))
            .join()
            .expect("controlled worker variants");
        set_query_scan_cancellation_after_entries(None);
        let disarmed = hook(&controlled);
        set_query_scan_cancellation_after_entries(Some(2));
        let rearmed = (hook(&controlled), hook(&controlled));
        set_query_scan_cancellation_after_entries(None);

        // Assert
        assert!(!before);
        assert_eq!(worker, (false, true));
        assert!(!disarmed);
        assert_eq!(rearmed, (false, true));
        assert!(!cancellation.is_canceled());
        assert!(cancellation.is_same_request(&cancellation.clone()));
    }

    #[test]
    fn should_preserve_outer_scope_after_nested_guard_rejection() {
        // Arrange
        let guard = query_scan_control_test_guard();
        let owned = controls();

        // Act
        let nested = std::panic::catch_unwind(query_scan_control_test_guard);
        set_query_scan_cancellation_after_entries(Some(2));
        let boundary = (hook(&owned), hook(&owned));
        set_query_scan_cancellation_after_entries(None);
        drop(guard);
        let _next = query_scan_control_test_guard();
        let next = controls();
        set_query_scan_cancellation_after_entries(Some(1));
        let next_boundary = hook(&next);
        set_query_scan_cancellation_after_entries(None);

        // Assert
        assert!(nested.is_err());
        assert_eq!(boundary, (false, true));
        assert!(next_boundary);
    }

    #[test]
    fn should_discard_unguarded_scope_after_disarm() {
        // Arrange
        set_query_scan_cancellation_after_entries(Some(2));
        let old = controls();
        set_query_scan_cancellation_after_entries(None);
        set_query_scan_cancellation_after_entries(Some(2));
        let new = controls();

        // Act
        let old = std::thread::spawn(move || (hook(&old), hook(&old)))
            .join()
            .expect("late unguarded scanner");
        let new = (hook(&new), hook(&new));
        set_query_scan_cancellation_after_entries(None);
        let cleared = super::super::current_query_scan_control_scope().is_none();

        // Assert
        assert_eq!(old, (false, false));
        assert_eq!(new, (false, true));
        assert!(cleared);
    }
}
