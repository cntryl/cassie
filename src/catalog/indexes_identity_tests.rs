use std::collections::BTreeMap;

use super::{IndexKind, IndexMeta, ReservationIdentity};

fn index() -> IndexMeta {
    IndexMeta {
        definition_semantics_version: 1,
        collection: "public.t".into(),
        name: "t_key".into(),
        field: "id".into(),
        fields: vec!["id".into()],
        expressions: Vec::new(),
        include_fields: Vec::new(),
        predicate: None,
        kind: IndexKind::Scalar,
        unique: true,
        options: BTreeMap::new(),
    }
}

#[test]
fn should_preserve_legacy_and_generated_unique_reservation_identity() {
    // Arrange
    let mut original = index();
    // Act / Assert
    assert!(matches!(
        original.reservation_identity(),
        Ok(ReservationIdentity::Legacy("t_key"))
    ));
    original.set_storage_ids(7, 42);
    original.set_reservation_generation(42).expect("generation");
    assert!(matches!(
        original.reservation_identity(),
        Ok(ReservationIdentity::Generation(42))
    ));
    assert!(original.read_definition_matches(&index()));
    original.name = "renamed".into();
    assert!(matches!(
        original.reservation_identity(),
        Ok(ReservationIdentity::Generation(42))
    ));
    let copied = original.clone_for_relation("public.other", "other_key");
    assert!(matches!(
        copied.reservation_identity(),
        Ok(ReservationIdentity::Legacy("other_key"))
    ));
}

#[test]
fn should_fail_closed_on_invalid_or_zero_persisted_reservation_generation() {
    // Arrange
    let mut original = index();
    // Act / Assert
    for invalid in ["0", "-1", "not-a-number", "18446744073709551616"] {
        original.options.insert(
            "__cassie_unique_reservation_generation".into(),
            invalid.into(),
        );
        assert!(original.reservation_identity().is_err(), "{invalid}");
    }
    assert!(original.set_reservation_generation(0).is_err());
    original.set_storage_ids(7, 42);
    assert!(original.set_reservation_generation(43).is_err());
}
