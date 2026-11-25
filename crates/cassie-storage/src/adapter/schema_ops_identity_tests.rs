use super::PendingFieldDrop;

#[test]
fn should_round_trip_reservation_generations_through_legacy_field_drop_record() {
    // Arrange
    let legacy = serde_json::json!({"collection":"public.t","field":"x","target_generation":2,"scalar_names":["legacy_idx"],"time_series_names":[]});
    // Act
    let mut pending: PendingFieldDrop = serde_json::from_value(legacy).expect("legacy record");
    let serialized_legacy = serde_json::to_value(&pending).expect("legacy serialize");
    pending.scalar_reservation_generations.push(42);
    let serialized = serde_json::to_value(&pending).expect("serialize");
    let decoded: PendingFieldDrop = serde_json::from_value(serialized).expect("decode");
    // Assert
    assert!(serialized_legacy
        .get("scalar_reservation_generations")
        .is_none());
    assert_eq!(decoded.scalar_names, vec!["legacy_idx"]);
    assert_eq!(decoded.scalar_reservation_generations, vec![42]);
}
