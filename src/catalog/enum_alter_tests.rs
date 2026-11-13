use super::AddValuePlan;
use crate::catalog::enum_types::{EnumTypeMeta, ENUM_TYPE_FORMAT_VERSION};
use crate::types::enum_type::{EnumMember, EnumValuePlacement};

fn member(member_id: u64, label: &str, sort_order: f32) -> EnumMember {
    EnumMember {
        member_id,
        label: label.to_owned(),
        sort_order,
        uncommitted: false,
    }
}

fn mood() -> EnumTypeMeta {
    EnumTypeMeta {
        format_version: ENUM_TYPE_FORMAT_VERSION,
        database: "db".to_owned(),
        name: "db.public.mood".to_owned(),
        object_id: 1,
        array_object_id: 2,
        owner_oid: 10,
        revision: 1,
        members: vec![member(11, "sad", 1.0), member(12, "happy", 2.0)],
    }
}

fn labels(meta: &EnumTypeMeta) -> Vec<&str> {
    meta.members
        .iter()
        .map(|member| member.label.as_str())
        .collect()
}

#[test]
fn should_append_a_trailing_value_with_the_next_sort_order() {
    // Arrange
    let current = mood();

    // Act
    let plan = current.plan_added_value("ok", None).expect("plan");
    let next = current.with_added_value(plan, "ok".to_owned(), 13, false);

    // Assert
    assert_eq!(labels(&next), ["sad", "happy", "ok"]);
    assert!((next.members[2].sort_order - 3.0).abs() < f32::EPSILON);
    assert_eq!(next.revision, 2);
}

#[test]
fn should_place_a_value_between_neighbours_at_the_midpoint() {
    // Arrange
    let current = mood();
    let placement = EnumValuePlacement::After("sad".to_owned());

    // Act
    let plan = current
        .plan_added_value("ok", Some(&placement))
        .expect("plan");
    let next = current.with_added_value(plan, "ok".to_owned(), 13, false);

    // Assert
    assert_eq!(labels(&next), ["sad", "ok", "happy"]);
    assert!((next.members[1].sort_order - 1.5).abs() < f32::EPSILON);
    assert_eq!(next.members[0].member_id, 11);
    assert_eq!(next.members[2].member_id, 12);
}

#[test]
fn should_place_a_value_before_the_first_member_below_its_sort_order() {
    // Arrange
    let current = mood();
    let placement = EnumValuePlacement::Before("sad".to_owned());

    // Act
    let plan = current
        .plan_added_value("ok", Some(&placement))
        .expect("plan");
    let next = current.with_added_value(plan, "ok".to_owned(), 13, false);

    // Assert
    assert_eq!(labels(&next), ["ok", "sad", "happy"]);
    assert!(next.members[0].sort_order.abs() < f32::EPSILON);
}

#[test]
fn should_renumber_every_member_when_no_midpoint_is_representable() {
    // Arrange
    let mut current = mood();
    current.members[0].sort_order = 1.0;
    current.members[1].sort_order = 1.0 + f32::EPSILON;
    let placement = EnumValuePlacement::After("sad".to_owned());

    // Act
    let plan = current
        .plan_added_value("ok", Some(&placement))
        .expect("plan");
    let next = current.with_added_value(plan, "ok".to_owned(), 13, false);

    // Assert
    assert_eq!(plan.sort_order, None);
    assert_eq!(labels(&next), ["sad", "ok", "happy"]);
    let orders = next
        .members
        .iter()
        .map(|member| member.sort_order)
        .collect::<Vec<_>>();
    assert_eq!(orders, vec![1.0, 2.0, 3.0]);
    assert_eq!(next.members[2].member_id, 12);
}

#[test]
fn should_reject_a_duplicate_label_before_checking_the_neighbour() {
    // Arrange
    let current = mood();
    let placement = EnumValuePlacement::After("missing".to_owned());

    // Act
    let error = current
        .plan_added_value("sad", Some(&placement))
        .expect_err("duplicate");

    // Assert
    assert_eq!(error.sqlstate, "42710");
    assert_eq!(error.message, "enum label \"sad\" already exists");
}

#[test]
fn should_reject_a_missing_neighbour_label() {
    // Arrange
    let current = mood();
    let placement = EnumValuePlacement::Before("missing".to_owned());

    // Act
    let error = current
        .plan_added_value("ok", Some(&placement))
        .expect_err("neighbour");

    // Assert
    assert_eq!(error.sqlstate, "22023");
    assert_eq!(error.message, "\"missing\" is not an existing enum label");
}

#[test]
fn should_rename_a_value_keeping_identity_and_position() {
    // Arrange
    let current = mood();

    // Act
    let next = current.with_renamed_value("sad", "blue").expect("rename");

    // Assert
    assert_eq!(labels(&next), ["blue", "happy"]);
    assert_eq!(next.members[0].member_id, 11);
    assert!((next.members[0].sort_order - 1.0).abs() < f32::EPSILON);
    assert_eq!(next.revision, 2);
}

#[test]
fn should_reject_renaming_onto_an_existing_label() {
    // Arrange
    let current = mood();

    // Act
    let error = current
        .with_renamed_value("sad", "happy")
        .expect_err("existing");

    // Assert
    assert_eq!(error.sqlstate, "42710");
}

#[test]
fn should_reject_renaming_a_missing_label() {
    // Arrange
    let current = mood();

    // Act
    let error = current
        .with_renamed_value("missing", "blue")
        .expect_err("missing");

    // Assert
    assert_eq!(error.sqlstate, "22023");
}

#[test]
fn should_bump_the_revision_when_renaming_the_type() {
    // Arrange
    let current = mood();

    // Act
    let next = current.with_name("db.archive.mood".to_owned());

    // Assert
    assert_eq!(next.name, "db.archive.mood");
    assert_eq!(next.revision, 2);
    assert_eq!(next.members, current.members);
}

#[test]
fn should_mark_a_new_value_uncommitted_only_when_requested() {
    // Arrange
    let current = mood();
    let plan = AddValuePlan {
        index: 2,
        sort_order: Some(3.0),
    };

    // Act
    let marked = current.with_added_value(plan, "ok".to_owned(), 13, true);
    let unmarked = current.with_added_value(plan, "ok".to_owned(), 13, false);

    // Assert
    assert!(marked.members[2].uncommitted);
    assert!(!unmarked.members[2].uncommitted);
}
