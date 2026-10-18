//! Measured source admission followed by the existing cumulative-retention law.

use super::support_pgwire as wire;
use super::support_portal_memory_calibration::{runtime, Fixture, Frames};

fn assert_suspended_page(frames: &Frames) {
    assert_eq!(wire::error_code(frames), None, "{frames:?}");
    let rows = wire::data_rows(frames);
    assert_eq!(rows.len(), 1, "one requested row: {frames:?}");
    assert_eq!(rows[0].len(), 1);
    assert!(rows[0][0].is_some());
    assert_eq!(frames.iter().filter(|frame| frame.0 == b's').count(), 1);
    assert_eq!(frames.iter().filter(|frame| frame.0 == b'C').count(), 0);
    assert_eq!(
        frames.last().map(|frame| frame.1.as_slice()),
        Some(&b"T"[..])
    );
}

fn calibrate_source_peak() -> usize {
    let fixture = Fixture::new("portal-shared-memory-calibration", None);
    let runtime = runtime();
    let peak = runtime.block_on(async {
        let mut client = fixture.connect().await;
        let setup = fixture.query();
        let first = client.page("memory_portal_one").await;
        assert_suspended_page(&first);
        let measured = fixture.query();
        let peak = usize::try_from(measured.peak_accounted_memory_bytes).expect("source peak fits usize");
        assert!(peak > 0 && peak <= fixture.budget());
        assert!(measured.peak_accounted_memory_bytes > setup.peak_accounted_memory_bytes);
        assert_eq!(measured.count, setup.count + 1);
        assert_eq!(measured.rows_returned_total, setup.rows_returned_total + 64);
        assert_eq!(measured.errors_total, setup.errors_total);
        println!("portal source calibration: setup_peak={} source_peak={peak} ample_budget={} source_count_delta=1 source_rows_delta=64 source_errors_delta=0", setup.peak_accounted_memory_bytes, fixture.budget());
        client.control("COMMIT", b'I').await;
        fixture.assert_committed_cleanup();
        client.stop().await;
        peak
    });
    drop(runtime);
    drop(fixture);
    peak
}

fn assert_retention_denial(frames: &Frames, budget: usize) -> usize {
    assert_eq!(wire::error_code(frames).as_deref(), Some("54000"));
    assert_eq!(wire::data_rows(frames), Vec::<Vec<Option<String>>>::new());
    assert_eq!(
        frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        b"2EZ"
    );
    assert_eq!(
        frames.last().map(|frame| frame.1.as_slice()),
        Some(&b"E"[..])
    );
    let (_, error) = frames
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("error frame");
    let fields = wire::parse_error_fields(error);
    let message = fields
        .iter()
        .find(|field| field.0 == 'M')
        .expect("memory diagnostic");
    let sizes = message
        .1
        .strip_prefix("query memory budget exceeded: ")
        .expect("specific budget diagnostic");
    let (requested, configured) = sizes
        .split_once(" > ")
        .expect("requested and configured bytes");
    let requested = requested.parse::<usize>().expect("requested charge");
    assert_eq!(
        configured.parse::<usize>().expect("configured budget"),
        budget
    );
    assert!(requested > budget);
    requested
}

#[test]
fn should_enforce_retained_memory_budget_across_named_portal_lifecycle() {
    // Arrange
    let peak = calibrate_source_peak();
    let fixture = Fixture::new("portal-shared-memory", Some(peak));
    let runtime = runtime();
    runtime.block_on(async {
        let mut client = fixture.connect().await;
        for portal in ["memory_portal_one", "memory_portal_two", "memory_portal_three"] {
            let frames = client.page(portal).await;
            assert_suspended_page(&frames);
            assert_eq!(fixture.query().peak_accounted_memory_bytes, u64::try_from(peak).expect("source peak fits u64"));
        }
        client.control("SAVEPOINT memory_probe", b'T').await;
        let before = fixture.query();

        // Act
        let overflow = client.page("memory_portal_four").await;
        let after = fixture.query();

        // Assert: successful source execution precedes the private retention denial.
        assert_eq!(after.count, before.count + 1);
        assert_eq!(after.rows_returned_total, before.rows_returned_total + 64);
        assert_eq!(after.errors_total, before.errors_total);
        assert_eq!(after.peak_accounted_memory_bytes, u64::try_from(peak).expect("source peak fits u64"));
        let requested = assert_retention_denial(&overflow, peak);
        println!("portal scarce lifecycle: source_budget={peak} source_peak={} fourth_requested={requested} source_count_delta=1 source_rows_delta=64 source_errors_delta=0", after.peak_accounted_memory_bytes);
        client.control("ROLLBACK TO memory_probe", b'T').await;
        // Error recovery alone must not make space before the explicit Close.
        let before_close = fixture.query();
        let still_full = client.page("memory_portal_before_close").await;
        let after_retry = fixture.query();
        assert_eq!(after_retry.count, before_close.count + 1);
        assert_eq!(after_retry.rows_returned_total, before_close.rows_returned_total + 64);
        assert_eq!(after_retry.errors_total, before_close.errors_total);
        assert_eq!(after_retry.peak_accounted_memory_bytes, u64::try_from(peak).expect("source peak fits u64"));
        let retry_requested = assert_retention_denial(&still_full, peak);
        println!("portal before-close probe: source_budget={peak} requested={retry_requested} source_count_delta=1 source_rows_delta=64 source_errors_delta=0");
        client.control("ROLLBACK TO memory_probe", b'T').await;
        let after_close = client.cycle(vec![
            wire::close_portal_frame("memory_portal_one"),
            wire::bind_frame("memory_portal_five", "memory_stmt", &[]),
            wire::execute_limited_frame("memory_portal_five", 1),
            wire::sync_frame(),
        ]).await;
        assert_suspended_page(&after_close);
        assert_eq!(after_close.iter().filter(|frame| frame.0 == b'3').count(), 1);
        assert_eq!(fixture.query().peak_accounted_memory_bytes, u64::try_from(peak).expect("source peak fits u64"));
        client.control("COMMIT", b'I').await;
        fixture.assert_committed_cleanup();
        client.stop().await;
    });
    drop(runtime);
    drop(fixture);
}
