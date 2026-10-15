use crate::support_pgwire as wire;
use crate::support_typed_portal::resume_after_change;

#[test]
fn should_keep_typed_materialized_portals_on_their_original_concurrent_snapshot() {
    // Arrange
    let mutations = [
        "INSERT INTO typed_portal (id, n) VALUES ('inserted', 7)",
        "DELETE FROM typed_portal WHERE n = 2",
        "UPDATE typed_portal SET id = 'rekeyed' WHERE n = 2",
    ];
    for mutation in mutations {
        // Act
        let pages = resume_after_change(false, mutation);

        // Assert
        assert_eq!(wire::error_code(&pages.first), None);
        assert!(pages.first.iter().any(|frame| frame.0 == b's'));
        assert_eq!(wire::error_code(&pages.second), None);
        let mut rows = wire::data_rows(&pages.first);
        rows.extend(wire::data_rows(&pages.second));
        rows.sort();
        assert_eq!(
            rows,
            (0..6)
                .map(|n| vec![Some(n.to_string())])
                .collect::<Vec<_>>()
        );
        assert_eq!(wire::error_code(&pages.closed).as_deref(), Some("26000"));
    }
}

#[test]
fn should_keep_initial_staged_rows_in_typed_portals_after_later_staged_changes() {
    // Arrange
    let mutations = [
        "INSERT INTO typed_portal (id, n) VALUES ('inserted', 7)",
        "DELETE FROM typed_portal WHERE n = 2",
        "UPDATE typed_portal SET id = 'rekeyed' WHERE n = 2",
    ];
    for mutation in mutations {
        // Act
        let pages = resume_after_change(true, mutation);

        // Assert
        assert_eq!(wire::error_code(&pages.first), None);
        assert_eq!(wire::error_code(&pages.second), None);
        let mut rows = wire::data_rows(&pages.first);
        rows.extend(wire::data_rows(&pages.second));
        rows.sort();
        assert_eq!(
            rows,
            (0..7)
                .map(|n| vec![Some(n.to_string())])
                .collect::<Vec<_>>()
        );
        assert_eq!(wire::error_code(&pages.closed).as_deref(), Some("26000"));
    }
}
