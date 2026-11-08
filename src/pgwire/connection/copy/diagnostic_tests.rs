use std::time::Instant;

use crate::app::CassieSession;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;

#[test]
fn should_close_copy_for_oversized_unknown_column_frame_without_writing() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let at_limit_session = CassieSession::new("root".into(), None);
    let above_limit_session = CassieSession::new("root".into(), None);
    at_limit_session.begin_transaction(None).expect("begin");
    above_limit_session.begin_transaction(None).expect("begin");
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let overhead =
        "unresolvable column reference '".len() + "'; known fields or aliases required".len() + 21;
    let max_name = super::super::writers::MAX_BACKEND_FRAME_BYTES - overhead;
    let mut writer = Vec::new();
    // Pass synthetic lengths to exercise the boundary without a 16 MiB fixture.
    // Act
    let (at_limit, above_limit, after_first) = runtime.block_on(async {
        let at_limit = super::write_copy_unknown_column_with_length(
            &mut writer,
            &at_limit_session,
            "x",
            max_name,
            &controls,
        )
        .await;
        let after_first = writer.len();
        let above_limit = super::write_copy_unknown_column_with_length(
            &mut writer,
            &above_limit_session,
            "x",
            max_name + 1,
            &controls,
        )
        .await;
        (at_limit, above_limit, after_first)
    });
    // Assert
    assert!(at_limit);
    assert!(!above_limit);
    assert!(after_first > 0);
    assert_eq!(writer.len(), after_first);
    assert_eq!(at_limit_session.transaction_status(), "failed");
    assert_eq!(above_limit_session.transaction_status(), "failed");
}
