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
fn should_resolve_unique_reservation_identity_across_generation_changes() {
    // Arrange
    let mut original = index();

    // Act
    let legacy_matches = matches!(
        original.reservation_identity(),
        Ok(ReservationIdentity::Legacy("t_key"))
    );
    original.set_storage_ids(7, 42);
    original.set_reservation_generation(42).expect("generation");
    let generated_matches = matches!(
        original.reservation_identity(),
        Ok(ReservationIdentity::Generation(42))
    );
    let definition_matches = original.read_definition_matches(&index());
    original.name = "renamed".into();
    let renamed_matches = matches!(
        original.reservation_identity(),
        Ok(ReservationIdentity::Generation(42))
    );
    let copied = original.clone_for_relation("public.other", "other_key");
    let copied_matches = matches!(
        copied.reservation_identity(),
        Ok(ReservationIdentity::Legacy("other_key"))
    );

    // Assert
    assert!(legacy_matches);
    assert!(generated_matches);
    assert!(definition_matches);
    assert!(renamed_matches);
    assert!(copied_matches);
}

#[test]
fn should_fail_closed_on_invalid_or_zero_persisted_reservation_generation() {
    // Arrange
    let mut original = index();

    // Act
    let invalid_results: Vec<_> = ["0", "-1", "not-a-number", "18446744073709551616"]
        .into_iter()
        .map(|invalid| {
            original.options.insert(
                "__cassie_unique_reservation_generation".into(),
                invalid.into(),
            );
            (invalid, original.reservation_identity().is_err())
        })
        .collect();
    let zero_rejected = original.set_reservation_generation(0).is_err();
    original.set_storage_ids(7, 42);
    let mismatch_rejected = original.set_reservation_generation(43).is_err();

    // Assert
    for (invalid, rejected) in invalid_results {
        assert!(rejected, "{invalid}");
    }
    assert!(zero_rejected);
    assert!(mismatch_rejected);
}
