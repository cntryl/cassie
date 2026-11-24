use super::*;

fn descriptor(labels: &[&str]) -> EnumType {
    EnumType {
        oid: 16_400,
        array_oid: 16_401,
        qualified_name: "postgres.public.state_code".to_string(),
        name: "state_code".to_string(),
        revision: 1,
        members: labels
            .iter()
            .enumerate()
            .map(|(position, label)| EnumMember {
                member_id: 20_000 + u64::try_from(position).expect("small position"),
                label: (*label).to_string(),
                sort_order: sort_order_at(position),
                uncommitted: false,
            })
            .collect(),
    }
}

#[test]
fn should_assign_postgresql_sort_orders_for_added_labels() {
    // Arrange
    let descriptor = descriptor(&["open", "closed"]);

    // Act
    let trailing = descriptor.sort_order_for(None);
    let before_closed =
        descriptor.sort_order_for(Some(&EnumValuePlacement::Before("closed".to_string())));
    let before_open =
        descriptor.sort_order_for(Some(&EnumValuePlacement::Before("open".to_string())));

    // Assert
    assert_eq!(trailing, Ok(Some(3.0)));
    assert_eq!(before_closed, Ok(Some(1.5)));
    assert_eq!(before_open, Ok(Some(0.0)));
}

#[test]
fn should_name_the_missing_neighbour_label() {
    // Arrange
    let descriptor = descriptor(&["open"]);

    // Act
    let outcome = descriptor.sort_order_for(Some(&EnumValuePlacement::After("nope".to_string())));

    // Assert
    assert_eq!(outcome, Err("nope".to_string()));
}

#[test]
fn should_request_renumbering_when_no_midpoint_is_representable() {
    // Arrange
    let mut descriptor = descriptor(&["a", "b"]);
    descriptor.members[1].sort_order =
        f32::from_bits(descriptor.members[0].sort_order.to_bits() + 1);

    // Act
    let outcome = descriptor.sort_order_for(Some(&EnumValuePlacement::Before("b".to_string())));
    let renumbered = descriptor.renumbered();

    // Assert
    assert_eq!(outcome, Ok(None));
    assert_eq!(
        renumbered
            .members
            .iter()
            .map(|member| (member.member_id, member.sort_order))
            .collect::<Vec<_>>(),
        vec![(20_000, 1.0), (20_001, 2.0)]
    );
}

#[test]
fn should_keep_value_identity_through_label_rename() {
    // Arrange
    let original = EnumRef::new(descriptor(&["open", "closed"]));
    let closed = EnumValue::from_label(&original, "closed").expect("closed label");
    let mut renamed = descriptor(&["open", "closed"]);
    renamed.members[1].label = "done".to_string();
    renamed.revision = 2;
    let renamed = EnumRef::new(renamed);

    // Act
    let reloaded = EnumValue::new(renamed.clone(), closed.member_id()).expect("same member");
    let open = EnumValue::from_label(&renamed, "open").expect("open label");

    // Assert
    assert_eq!(reloaded.label(), "done");
    assert!(open.order_key() < reloaded.order_key());
    assert!(EnumValue::from_label(&renamed, "closed").is_none());
}

#[test]
fn should_round_trip_the_stored_json_form() {
    // Arrange
    let descriptor = EnumRef::new(descriptor(&["open", "closed"]));
    let value = EnumValue::from_label(&descriptor, "closed").expect("closed label");

    // Act
    let stored = value.to_storage_json();
    let member_id = EnumValue::storage_member_id(&stored);

    // Assert
    assert_eq!(stored, serde_json::json!({ "$cassie_enum_v1": 20_001 }));
    assert_eq!(member_id, Some(20_001));
    assert_eq!(
        EnumValue::storage_member_id(&serde_json::json!("closed")),
        None
    );
}

#[test]
fn should_reject_member_ids_outside_the_descriptor() {
    // Arrange
    let descriptor = EnumRef::new(descriptor(&["open"]));

    // Act
    let value = EnumValue::new(descriptor, 99);

    // Assert
    assert!(value.is_none());
}

#[test]
fn should_reject_unusable_labels_on_sql_input() {
    // Arrange
    let mut pending = descriptor(&["a", "b"]);
    pending.members[1].uncommitted = true;
    let pending = EnumRef::new(pending);

    // Act
    let committed = EnumValue::parse_label(&pending, "a");
    let uncommitted = EnumValue::parse_label(&pending, "b").map(|value| value.member_id());
    let unknown = EnumValue::parse_label(&pending, "z").map(|value| value.member_id());

    // Assert
    assert_eq!(committed.map(|value| value.member_id()), Ok(20_000));
    assert_eq!(
        uncommitted.map_err(|error| (error.sqlstate(), error.message())),
        Err((
            "55P04",
            "unsafe use of new value \"b\" of enum type state_code".to_string()
        ))
    );
    assert_eq!(
        unknown.map_err(|error| (error.sqlstate(), error.message())),
        Err((
            "22P02",
            "invalid input value for enum state_code: \"z\"".to_string()
        ))
    );
}

#[test]
fn should_not_persist_the_uncommitted_marker() {
    // Arrange
    let member = EnumMember {
        member_id: 7,
        label: "b".to_string(),
        sort_order: 1.5,
        uncommitted: true,
    };

    // Act
    let stored = serde_json::to_value(&member).expect("serialize member");
    let reloaded: EnumMember = serde_json::from_value(stored.clone()).expect("reload member");

    // Assert
    assert_eq!(
        stored,
        serde_json::json!({"member_id": 7, "label": "b", "sort_order": 1.5})
    );
    assert!(!reloaded.uncommitted);
}
