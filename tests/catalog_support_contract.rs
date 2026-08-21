#[test]
fn should_publish_the_stable_named_client_catalog_subset() {
    // Arrange
    let feature_support = include_str!("../docs/feature-support.md");
    let catalog_contract = include_str!("../docs/catalog-support.md");
    let readiness = include_str!("../docs/production-readiness.md");
    let evidence = include_str!("../docs/promotion-evidence-matrix.md");

    // Act
    let named_clients = ["sqlx 0.8.3", "Diesel 2.2.6"];
    let stable_views = ["information_schema.tables", "information_schema.columns"];

    // Assert
    for view in stable_views {
        assert!(feature_support.contains(&format!("| `{view}` |")));
        assert!(catalog_contract.contains(&format!("### `{view}`")));
    }
    for client in named_clients {
        assert!(catalog_contract.contains(client));
    }
    assert!(catalog_contract.contains("explicit `ORDER BY`"));
    assert!(catalog_contract.contains("SQLSTATE `42P01`"));
    assert!(catalog_contract.contains("PostgreSQL-internal catalog parity is not claimed"));
    assert!(readiness.contains("stable named-client catalog subset"));
    assert!(evidence.contains("Named-client catalog subset promoted"));
}
