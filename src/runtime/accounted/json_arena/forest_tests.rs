use std::time::Instant;

use super::super::cursor::Cursor;
use super::{Forest, Owner, Progress};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_move_one_unattached_child_with_detach_the_replaced_subtree() {
    // Arrange
    let controls = controls();
    let mut forest = Forest::new(&controls).expect("forest");
    let parent = forest.object().expect("parent");
    let first = forest.scalar().expect("first");
    let second = forest.scalar().expect("second");
    forest.insert(parent, "key", first).expect("first edge");

    // Act
    let displaced = forest.insert(parent, "key", second).expect("replacement");

    // Assert
    assert_eq!(displaced, Some(first));
    assert_eq!(forest.parent(first).expect("first"), Owner::Unattached);
    assert_eq!(
        forest.parent(second).expect("second"),
        Owner::Parent(parent)
    );
    assert_eq!(forest.member(parent, "key").expect("lookup"), Some(second));
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_duplicate_attachment_with_ancestor_cycles() {
    // Arrange
    let controls = controls();
    let mut forest = Forest::new(&controls).expect("forest");
    let outer = forest.object().expect("outer");
    let inner = forest.object().expect("inner");
    let child = forest.scalar().expect("child");
    forest.insert(outer, "inner", inner).expect("edge");
    forest.insert(inner, "child", child).expect("edge");

    // Act / Assert
    // Assert
    assert!(forest.insert(inner, "self", inner).is_err());
    assert!(forest.insert(inner, "cycle", outer).is_err());
    assert!(forest.insert(outer, "again", child).is_err());
    assert_eq!(forest.parent(outer).expect("outer"), Owner::Unattached);
    assert_eq!(forest.parent(inner).expect("inner"), Owner::Parent(outer));
    assert_eq!(forest.parent(child).expect("child"), Owner::Parent(inner));
    assert_eq!(forest.member(inner, "child").expect("lookup"), Some(child));
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_a_root_until_it_is_explicitly_detached() {
    // Arrange
    let controls = controls();
    let mut forest = Forest::new(&controls).expect("forest");
    let parent = forest.object().expect("parent");
    let child = forest.scalar().expect("child");
    forest.mark_root(child).expect("root");

    // Act / Assert
    // Assert
    assert!(forest.insert(parent, "child", child).is_err());
    assert_eq!(forest.parent(child).expect("child"), Owner::Root);
    assert_eq!(forest.member(parent, "child").expect("lookup"), None);
    forest.detach_root(child).expect("detach");
    forest.insert(parent, "child", child).expect("attach");
    assert_eq!(forest.parent(child).expect("child"), Owner::Parent(parent));
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_cross_arena_with_stale_nodes_before_mutation() {
    // Arrange
    let controls = controls();
    let mut first = Forest::new(&controls).expect("first");
    let mut second = Forest::new(&controls).expect("second");
    let parent = first.object().expect("parent");
    let foreign = second.scalar().expect("foreign");
    let retired = first.scalar().expect("retired");
    first.retire_empty(retired).expect("retire");
    let replacement = first.scalar().expect("replacement");

    // Act / Assert
    // Assert
    assert!(first.insert(parent, "foreign", foreign).is_err());
    assert!(first.insert(parent, "stale", retired).is_err());
    assert_eq!(
        first.parent(replacement).expect("replacement"),
        Owner::Unattached
    );
    assert_eq!(first.member(parent, "foreign").expect("lookup"), None);
    assert_eq!(first.member(parent, "stale").expect("lookup"), None);
    drop(first);
    drop(second);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_edges_when_cancelled_during_parent_walk() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut forest = Forest::new(&controls).expect("forest");
    let outer = forest.object().expect("outer");
    let inner = forest.object().expect("inner");
    let child = forest.scalar().expect("child");
    forest.insert(outer, "inner", inner).expect("edge");

    // Act
    let result = forest.insert_observed(inner, "child", child, |progress| {
        if matches!(progress, Progress::ParentWalk) {
            cancellation.cancel();
        }
    });

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(forest.parent(child).expect("child"), Owner::Unattached);
    assert_eq!(forest.parent(inner).expect("inner"), Owner::Parent(outer));
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_duplicate_edge_when_cancelled_before_insertion() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut forest = Forest::new(&controls).expect("forest");
    let parent = forest.object().expect("parent");
    let old = forest.scalar().expect("old");
    let replacement = forest.scalar().expect("replacement");
    forest.insert(parent, "key", old).expect("edge");

    // Act
    let result = forest.insert_observed(parent, "key", replacement, |progress| {
        if matches!(progress, Progress::BeforeInsert) {
            cancellation.cancel();
        }
    });

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(forest.parent(old).expect("old"), Owner::Parent(parent));
    assert_eq!(
        forest.parent(replacement).expect("replacement"),
        Owner::Unattached
    );
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_edge_with_candidate_unmodified_when_key_admission_fails() {
    // Arrange
    let limit = 4096;
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: limit,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let mut forest = Forest::new(&controls).expect("forest");
    let parent = forest.object().expect("parent");
    let old = forest.scalar().expect("old");
    let replacement = forest.scalar().expect("replacement");
    forest.insert(parent, "key", old).expect("edge");
    let blocker = controls
        .reserve_query_memory(limit - controls.current_query_memory_bytes())
        .expect("reserve remaining budget");

    // Act
    let result = forest.insert(parent, "key", replacement);

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(forest.parent(old).expect("old"), Owner::Parent(parent));
    assert_eq!(
        forest.parent(replacement).expect("replacement"),
        Owner::Unattached
    );
    drop(blocker);
    assert_eq!(forest.member(parent, "key").expect("lookup"), Some(old));
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retire_a_displaced_subtree_without_touching_survivors() {
    // Arrange
    let controls = controls();
    let mut forest = Forest::new(&controls).expect("forest");
    let parent = forest.object().expect("parent");
    let old = forest.object().expect("old");
    let nested = forest.object().expect("nested");
    let leaf = forest.scalar().expect("leaf");
    let replacement = forest.scalar().expect("replacement");
    forest.insert(nested, "leaf", leaf).expect("leaf edge");
    forest.insert(old, "nested", nested).expect("nested edge");
    forest.insert(parent, "key", old).expect("old edge");
    assert_eq!(
        forest.insert(parent, "key", replacement).expect("replace"),
        Some(old)
    );

    // Act
    forest.retire_subtree(old).expect("retire");

    // Assert
    assert!(forest.parent(old).is_err());
    assert!(forest.parent(nested).is_err());
    assert!(forest.parent(leaf).is_err());
    assert_eq!(
        forest.parent(replacement).expect("replacement"),
        Owner::Parent(parent)
    );
    assert_eq!(
        forest.member(parent, "key").expect("member"),
        Some(replacement)
    );
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_retiring_attached_with_stale_roots() {
    // Arrange
    let controls = controls();
    let mut forest = Forest::new(&controls).expect("forest");
    let parent = forest.object().expect("parent");
    let child = forest.scalar().expect("child");
    let stale = forest.scalar().expect("stale");
    forest.insert(parent, "child", child).expect("edge");
    forest.retire_empty(stale).expect("retire");

    // Act / Assert
    // Assert
    assert!(forest.retire_subtree(child).is_err());
    assert!(forest.retire_subtree(stale).is_err());
    assert_eq!(forest.member(parent, "child").expect("member"), Some(child));
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_subtree_intact_when_retirement_planning_is_cancelled() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut forest = Forest::new(&controls).expect("forest");
    let root = forest.object().expect("root");
    let child = forest.scalar().expect("child");
    forest.insert(root, "child", child).expect("edge");

    // Act
    let result = forest.retire_observed(root, |progress| {
        if matches!(progress, Progress::RetirementVisit) {
            cancellation.cancel();
        }
    });

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(forest.parent(root).expect("root"), Owner::Unattached);
    assert_eq!(forest.parent(child).expect("child"), Owner::Parent(root));
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_subtree_intact_when_retirement_plan_admission_is_denied() {
    // Arrange
    let limit = 4096;
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: limit,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let mut forest = Forest::new(&controls).expect("forest");
    let root = forest.object().expect("root");
    let child = forest.scalar().expect("child");
    forest.insert(root, "child", child).expect("edge");
    let blocker = controls
        .reserve_query_memory(limit - controls.current_query_memory_bytes())
        .expect("blocker");

    // Act
    let result = forest.retire_subtree(root);

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(forest.parent(root).expect("root"), Owner::Unattached);
    assert_eq!(forest.parent(child).expect("child"), Owner::Parent(root));
    drop(blocker);
    assert_eq!(forest.member(root, "child").expect("member"), Some(child));
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_subtree_intact_when_cancelled_while_growing_retirement_plan() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut forest = Forest::new(&controls).expect("forest");
    let root = forest.object().expect("root");
    let mut children = Vec::new();
    for key in ["a", "b", "c", "d", "e"] {
        let child = forest.scalar().expect("child");
        forest.insert(root, key, child).expect("edge");
        children.push(child);
    }

    // Act
    let result = forest.retire_observed(root, |progress| {
        if matches!(progress, Progress::RetirementMove) {
            cancellation.cancel();
        }
    });

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(forest.parent(root).expect("root"), Owner::Unattached);
    for child in children {
        assert_eq!(forest.parent(child).expect("child"), Owner::Parent(root));
    }
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reuse_retired_node_slots_without_reviving_old_handles() {
    // Arrange
    let controls = controls();
    let mut forest = Forest::new(&controls).expect("forest");
    let root = forest.object().expect("root");
    let child = forest.scalar().expect("child");
    forest.insert(root, "child", child).expect("edge");
    forest.retire_subtree(root).expect("retire");
    let retained = controls.current_query_memory_bytes();

    // Act
    let replacement = forest.scalar().expect("reuse");

    // Assert
    assert!(forest.parent(root).is_err());
    assert!(forest.parent(child).is_err());
    assert_eq!(
        forest.parent(replacement).expect("replacement"),
        Owner::Unattached
    );
    assert_eq!(controls.current_query_memory_bytes(), retained);
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_array_order_with_reject_repeated_or_cyclic_edges() {
    // Arrange
    let controls = controls();
    let mut forest = Forest::new(&controls).expect("forest");
    let outer = forest.array().expect("outer");
    let inner = forest.array().expect("inner");
    let child = forest.scalar().expect("child");
    forest.append(outer, inner).expect("outer edge");
    forest.append(inner, child).expect("child edge");

    // Act / Assert
    // Assert
    assert!(forest.append(inner, outer).is_err());
    assert!(forest.append(inner, inner).is_err());
    assert!(forest.append(outer, child).is_err());
    assert_eq!(forest.element(outer, 0).expect("outer"), Some(inner));
    assert_eq!(forest.element(inner, 0).expect("inner"), Some(child));
    assert_eq!(forest.element(inner, 1).expect("missing"), None);
    assert_eq!(forest.parent(child).expect("child"), Owner::Parent(inner));
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_stale_with_foreign_array_children() {
    // Arrange
    let controls = controls();
    let mut first = Forest::new(&controls).expect("first");
    let mut second = Forest::new(&controls).expect("second");
    let array = first.array().expect("array");
    let stale = first.scalar().expect("stale");
    let foreign = second.scalar().expect("foreign");
    first.retire_empty(stale).expect("retire");
    let replacement = first.scalar().expect("replacement");

    // Act / Assert
    // Assert
    assert!(first.append(array, stale).is_err());
    assert!(first.append(array, foreign).is_err());
    assert_eq!(first.element(array, 0).expect("empty"), None);
    assert_eq!(
        first.parent(replacement).expect("replacement"),
        Owner::Unattached
    );
    drop(first);
    drop(second);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retire_mixed_array_with_object_descendants() {
    // Arrange
    let controls = controls();
    let mut forest = Forest::new(&controls).expect("forest");
    let root = forest.array().expect("root");
    let object = forest.object().expect("object");
    let child = forest.scalar().expect("child");
    let survivor = forest.scalar().expect("survivor");
    forest.insert(object, "child", child).expect("object edge");
    forest.append(root, object).expect("array edge");

    // Act
    forest.retire_subtree(root).expect("retire");

    // Assert
    assert!(forest.parent(root).is_err());
    assert!(forest.parent(object).is_err());
    assert!(forest.parent(child).is_err());
    assert_eq!(
        forest.parent(survivor).expect("survivor"),
        Owner::Unattached
    );
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_array_unchanged_when_cancelled_during_directory_growth() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut forest = Forest::new(&controls).expect("forest");
    let array = forest.array().expect("array");
    let mut children = Vec::new();
    for _ in 0..4 {
        let child = forest.scalar().expect("child");
        forest.append(array, child).expect("edge");
        children.push(child);
    }
    let candidate = forest.scalar().expect("candidate");

    // Act
    let result = forest.append_observed(array, candidate, |progress| {
        if matches!(progress, Progress::ArrayMove) {
            cancellation.cancel();
        }
    });

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    for (index, child) in children.into_iter().enumerate() {
        assert_eq!(forest.element(array, index).expect("element"), Some(child));
        assert_eq!(forest.parent(child).expect("child"), Owner::Parent(array));
    }
    assert_eq!(forest.element(array, 4).expect("missing"), None);
    assert_eq!(
        forest.parent(candidate).expect("candidate"),
        Owner::Unattached
    );
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_array_unchanged_when_growth_admission_is_denied() {
    // Arrange
    let limit = 4096;
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: limit,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let mut forest = Forest::new(&controls).expect("forest");
    let array = forest.array().expect("array");
    let candidate = forest.scalar().expect("candidate");
    let blocker = controls
        .reserve_query_memory(limit - controls.current_query_memory_bytes())
        .expect("blocker");

    // Act
    let result = forest.append(array, candidate);

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(forest.element(array, 0).expect("missing"), None);
    assert_eq!(
        forest.parent(candidate).expect("candidate"),
        Owner::Unattached
    );
    drop(blocker);
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_own_scalar_payloads_through_mixed_tree_retirement() {
    // Arrange
    let controls = controls();
    let mut forest = Forest::new(&controls).expect("forest");
    let root = forest.array().expect("root");
    let null = forest.scalar().expect("null");
    let boolean = forest.boolean(true).expect("boolean");
    let input = format!("{}🙂é", "x".repeat(1023));
    let text = forest.text_node(&input).expect("text");
    forest.append(root, null).expect("null edge");
    forest.append(root, boolean).expect("boolean edge");
    forest.append(root, text).expect("text edge");
    let before = controls.current_query_memory_bytes();

    // Act
    assert_eq!(forest.boolean_value(boolean).expect("boolean"), Some(true));
    assert_eq!(forest.text_value(text).expect("text"), Some(input.as_str()));
    forest.retire_subtree(root).expect("retire");

    // Assert
    assert!(forest.parent(text).is_err());
    assert!(before - controls.current_query_memory_bytes() >= input.len());
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_drop_partial_text_with_its_lease_after_actual_copy_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut forest = Forest::new(&controls).expect("forest");
    let survivor = forest.scalar().expect("survivor");
    let baseline = controls.current_query_memory_bytes();
    let input = "é".repeat(513);

    // Act
    let result = forest.text_node_observed(&input, |progress| {
        if matches!(progress, Progress::TextCopy) {
            cancellation.cancel();
        }
    });

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(
        forest.parent(survivor).expect("survivor"),
        Owner::Unattached
    );
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_forest_unchanged_when_text_admission_is_denied() {
    // Arrange
    let limit = 4096;
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: limit,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let mut forest = Forest::new(&controls).expect("forest");
    let survivor = forest.scalar().expect("survivor");
    let blocker = controls
        .reserve_query_memory(limit - controls.current_query_memory_bytes())
        .expect("blocker");

    // Act
    let result = forest.text_node("payload");

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(
        forest.parent(survivor).expect("survivor"),
        Owner::Unattached
    );
    drop(blocker);
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_with_retire_an_empty_string() {
    // Arrange
    let controls = controls();
    let mut forest = Forest::new(&controls).expect("forest");

    // Act
    let node = forest.text_node("").expect("empty text");

    // Assert
    assert_eq!(forest.text_value(node).expect("text"), Some(""));
    forest.retire_subtree(node).expect("retire");
    assert!(forest.parent(node).is_err());
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_copied_text_if_following_node_growth_is_denied() {
    // Arrange
    let limit = 16384;
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: limit,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let mut forest = Forest::new(&controls).expect("forest");
    let mut ids = Vec::new();
    for _ in 0..16 {
        ids.push(forest.scalar().expect("node"));
    }
    let baseline = controls.current_query_memory_bytes();
    let blocker = controls
        .reserve_query_memory(limit - baseline - 3)
        .expect("blocker");

    // Act
    let result = forest.text_node("txt");

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(controls.current_query_memory_bytes(), limit - 3);
    for id in ids {
        assert_eq!(forest.parent(id).expect("live"), Owner::Unattached);
    }
    drop(blocker);
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_adopt_string_with_number_tokens_without_copying_or_changing_tags() {
    // Arrange
    let controls = controls();
    let mut forest = Forest::new(&controls).expect("forest");
    let mut string_cursor = Cursor::from_str("\"abc\"", &controls);
    let mut number_cursor = Cursor::from_str("1E2", &controls);
    let string = string_cursor.read_string().expect("string");
    let number = number_cursor.read_number().expect("number");

    // Act
    let text = forest.adopt_text(string).expect("adopt text");
    let numeric = forest.adopt_number(number).expect("adopt number");

    // Assert
    assert_eq!(forest.text_value(text).expect("text"), Some("abc"));
    assert_eq!(forest.number_value(numeric).expect("number"), Some("1e+2"));
    assert_eq!(
        forest.text_value(numeric).expect("number is not text"),
        None
    );
    forest.retire_subtree(text).expect("retire text");
    forest.retire_subtree(numeric).expect("retire number");
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_foreign_tracker_token_with_release_its_payload() {
    // Arrange
    let owner_controls = controls();
    let other_controls = controls();
    let mut forest = Forest::new(&owner_controls).expect("forest");
    let mut cursor = Cursor::from_str("\"abc\"", &other_controls);
    let token = cursor.read_string().expect("token");

    // Act
    let result = forest.adopt_text(token);

    // Assert
    assert!(matches!(result, Err(CassieError::Execution(_))));
    assert_eq!(other_controls.current_query_memory_bytes(), 0);
    assert_eq!(owner_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_existing_key_with_release_new_key_after_token_adoption() {
    // Arrange
    let controls = controls();
    let mut forest = Forest::new(&controls).expect("forest");
    let parent = forest.object().expect("parent");
    let first = forest.scalar().expect("first");
    let replacement = forest.scalar().expect("replacement");
    let mut first_cursor = Cursor::from_str("\"key\"", &controls);
    forest
        .insert_decoded(parent, first_cursor.read_string().expect("key"), first)
        .expect("edge");
    let baseline = controls.current_query_memory_bytes();
    let mut second_cursor = Cursor::from_str("\"key\"", &controls);
    let key = second_cursor.read_string().expect("key");

    // Act
    let old = forest
        .insert_decoded(parent, key, replacement)
        .expect("replace");

    // Assert
    assert_eq!(old, Some(first));
    assert_eq!(
        forest.member(parent, "key").expect("lookup"),
        Some(replacement)
    );
    assert_eq!(forest.parent(first).expect("first"), Owner::Unattached);
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_a_foreign_key_token_before_attaching_its_candidate() {
    // Arrange
    let owner_controls = controls();
    let other_controls = controls();
    let mut forest = Forest::new(&owner_controls).expect("forest");
    let parent = forest.object().expect("parent");
    let candidate = forest.scalar().expect("candidate");
    let mut cursor = Cursor::from_str("\"key\"", &other_controls);
    let key = cursor.read_string().expect("key");

    // Act
    let result = forest.insert_decoded(parent, key, candidate);

    // Assert
    assert!(matches!(result, Err(CassieError::Execution(_))));
    assert_eq!(
        forest.parent(candidate).expect("candidate"),
        Owner::Unattached
    );
    assert_eq!(forest.member(parent, "key").expect("lookup"), None);
    assert_eq!(other_controls.current_query_memory_bytes(), 0);
    drop(forest);
    assert_eq!(owner_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_adopted_payload_when_node_growth_is_denied() {
    // Arrange
    let limit = 16384;
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: limit,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let mut forest = Forest::new(&controls).expect("forest");
    for _ in 0..16 {
        forest.scalar().expect("node");
    }
    let mut cursor = Cursor::from_str("\"abc\"", &controls);
    let token = cursor.read_string().expect("token");
    let blocker = controls
        .reserve_query_memory(limit - controls.current_query_memory_bytes())
        .expect("blocker");

    // Act
    let result = forest.adopt_text(token);

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(controls.current_query_memory_bytes(), limit - 4);
    drop(blocker);
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_adopted_payload_when_cancelled_before_node_insertion() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut forest = Forest::new(&controls).expect("forest");
    let mut cursor = Cursor::from_str("\"abc\"", &controls);
    let token = cursor.read_string().expect("token");

    // Act
    let result = forest.adopt_text_observed(token, || cancellation.cancel());

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_a_validated_number_token_from_another_tracker() {
    // Arrange
    let owner_controls = controls();
    let other_controls = controls();
    let mut forest = Forest::new(&owner_controls).expect("forest");
    let mut cursor = Cursor::from_str("1E2", &other_controls);
    let token = cursor.read_number().expect("number");

    // Act
    let result = forest.adopt_number(token);

    // Assert
    assert!(matches!(result, Err(CassieError::Execution(_))));
    assert_eq!(other_controls.current_query_memory_bytes(), 0);
    assert_eq!(owner_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_distinct_validated_number_spellings_after_adoption() {
    // Arrange
    let controls = controls();
    let mut forest = Forest::new(&controls).expect("forest");
    let mut ids = Vec::new();

    // Act
    for input in ["1", "1.0", "1e0", "-0"] {
        let mut cursor = Cursor::from_str(input, &controls);
        ids.push(
            forest
                .adopt_number(cursor.read_number().expect("number"))
                .expect("adopt"),
        );
    }

    // Assert
    for (id, expected) in ids.into_iter().zip(["1", "1.0", "1e+0", "-0"]) {
        assert_eq!(forest.number_value(id).expect("value"), Some(expected));
    }
    drop(forest);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_validated_number_after_cancellation_during_adoption() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut forest = Forest::new(&controls).expect("forest");
    let mut cursor = Cursor::from_str("123", &controls);
    let token = cursor.read_number().expect("number");

    // Act
    let result = forest.adopt_number_observed(token, || cancellation.cancel());

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
