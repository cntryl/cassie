#[test]
fn should_publish_the_narrow_procedure_support_contract() {
    // Arrange
    let contract = include_str!("../docs/procedure-support.md");
    let feature_support = include_str!("../docs/feature-support.md");

    // Act
    let required_boundaries = [
        "single Cassie SQL statement",
        "positional argument binding",
        "restart hydration",
        "tokio-postgres",
        "PL/pgSQL",
        "triggers",
        "dynamic SQL",
        "transaction control",
        "recursion",
        "business-logic platform",
    ];
    let missing = required_boundaries
        .into_iter()
        .filter(|boundary| !contract.contains(boundary))
        .collect::<Vec<_>>();

    // Assert
    assert!(
        missing.is_empty(),
        "missing procedure boundaries: {missing:?}"
    );
    assert!(feature_support.contains("| Limited procedures and `CALL`"));
}

#[test]
fn should_reject_unsupported_procedural_surfaces() {
    // Arrange
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-procedure-boundary-{}", Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir_and_config(&path, CassieRuntimeConfig::default())
        .expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    let unsupported = [
        r"CREATE PROCEDURE procedural() LANGUAGE plpgsql AS 'BEGIN NULL; END'",
        "CREATE TRIGGER procedure_trigger BEFORE INSERT ON target EXECUTE PROCEDURE procedural()",
        r#"CREATE PROCEDURE dynamic_query() AS "EXECUTE 'SELECT 1'""#,
    ];

    // Act
    let errors = unsupported
        .into_iter()
        .map(|sql| {
            cassie
                .execute_sql(&session, sql, vec![])
                .expect_err("unsupported procedure surface must be rejected")
                .to_string()
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(errors.iter().all(|error| !error.trim().is_empty()));
    assert!(cassie.catalog.list_procedures().is_empty());

    cassie.shutdown();
    let _ = std::fs::remove_dir_all(path);
}
use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use uuid::Uuid;
