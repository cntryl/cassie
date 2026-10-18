#[test]
fn should_describe_trigger_metadata_without_trigger_objects() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-information-schema-triggers-empty",
        &["CREATE TABLE issue_817_trigger_metadata (id INTEGER)"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT trigger_catalog, trigger_schema, trigger_name, event_manipulation, \
                    event_object_catalog, event_object_schema, event_object_table, \
                    action_orientation, action_timing, created \
             FROM information_schema.triggers \
             WHERE event_object_schema = 'public' \
               AND event_object_table = 'issue_817_trigger_metadata' \
             ORDER BY trigger_name, event_manipulation",
        )
        .expect("describe trigger metadata for a table with no triggers");

    // Assert
    assert_descriptor(
        &selected,
        &[
            ("trigger_catalog", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("trigger_schema", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("trigger_name", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("event_manipulation", PG18_CHARACTER_DATA_OID, -1, -1, true),
            (
                "event_object_catalog",
                PG18_SQL_IDENTIFIER_OID,
                64,
                -1,
                true,
            ),
            ("event_object_schema", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("event_object_table", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("action_orientation", PG18_CHARACTER_DATA_OID, -1, -1, true),
            ("action_timing", PG18_CHARACTER_DATA_OID, -1, -1, true),
            ("created", PG18_TIME_STAMP_OID, 8, -1, true),
        ],
    );
    assert_eq!(selected.rows(), Vec::<Vec<cassie::types::Value>>::new());
}
