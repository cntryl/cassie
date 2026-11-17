//! Tiny source-phase ownership probes, before outer projection/final-output accounting.

use super::*;
use crate::executor::execution::graph_memory_test_observer as observer;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{Expr, FunctionCall};
use crate::types::graph::{Direction, ElementKind};
use crate::types::SqlArray;
use std::collections::HashMap;
use std::time::Instant;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
    )
}

fn reference() -> ElementRef {
    ElementRef {
        database_id: uuid::Uuid::from_u128(1),
        graph_id: uuid::Uuid::from_u128(2),
        mapping_id: uuid::Uuid::from_u128(3),
        kind: ElementKind::Vertex,
        key: vec![KeyPart::Text("tiny-key".to_owned())],
    }
}

fn assert_source_ownership(local: &str, admission: bool) {
    // Arrange
    let (cassie, session) = observer::fixture();
    let descriptors =
        crate::executor::filter::graph_functions::descriptors_of(Some(&session)).expect("types");
    let vertex = reference();
    let edge = ElementRef {
        kind: ElementKind::Edge,
        mapping_id: uuid::Uuid::from_u128(4),
        ..vertex.clone()
    };
    let path = Path {
        vertices: vec![vertex.clone(), vertex.clone()],
        edges: vec![edge],
        directions: vec![Direction::Forward],
    };
    let params = match local {
        "key_parts" => vec![vertex.to_value(&descriptors).expect("reference")],
        "path_steps" => vec![path.to_value(&descriptors).expect("path")],
        "weighted_path_steps" => vec![
            path.to_value(&descriptors).expect("path"),
            Value::Array(SqlArray::from_elements(vec![Value::Int64(2)]).expect("cost array")),
        ],
        _ => panic!("known graph source"),
    };
    let functions = HashMap::new();
    let controls = controls();
    let _observation = observer::start();
    observer::expect_owner(&controls);
    let name = format!("cassie_graph.{local}");
    let function = FunctionCall {
        name: name.clone(),
        args: (0..params.len()).map(Expr::Param).collect(),
        ..FunctionCall::default()
    };
    let env = SourceExecutionEnv {
        cassie: &cassie,
        session: Some(&session),
        user_functions: &functions,
        params: &params,
        parameter_type_oids: &[],
        controls: &controls,
    };
    let base = controls.current_query_memory_bytes();

    // Act
    let result = execute(&env, &name, &function, None, false).expect("expand native graph value");
    let retained = controls.current_query_memory_bytes();
    drop(result);
    let released = controls.current_query_memory_bytes();

    // Assert
    if admission {
        observer::assert_admitted(
            "before_graph_rows",
            "copy_graph_rows",
            std::mem::size_of::<BatchRow>(),
        );
    } else {
        assert!(
            retained >= base + std::mem::size_of::<BatchRow>(),
            "{local} must return owned rows"
        );
        assert_eq!(
            released, base,
            "{local} owner ends with the returned source batches"
        );
    }
}

#[test]
fn should_admit_key_part_rows_before_copying() {
    // Arrange
    let source = "key_parts";
    // Act
    // Assert
    assert_source_ownership(source, true);
}

#[test]
fn should_retain_the_key_part_owner_until_source_batches_drop() {
    // Arrange
    let source = "key_parts";
    // Act
    // Assert
    assert_source_ownership(source, false);
}

#[test]
fn should_admit_path_step_rows_before_copying() {
    // Arrange
    let source = "path_steps";
    // Act
    // Assert
    assert_source_ownership(source, true);
}

#[test]
fn should_retain_the_path_step_owner_until_source_batches_drop() {
    // Arrange
    let source = "path_steps";
    // Act
    // Assert
    assert_source_ownership(source, false);
}

#[test]
fn should_admit_weighted_path_step_rows_before_copying() {
    // Arrange
    let source = "weighted_path_steps";
    // Act
    // Assert
    assert_source_ownership(source, true);
}

#[test]
fn should_retain_the_weighted_path_step_owner_until_source_batches_drop() {
    // Arrange
    let source = "weighted_path_steps";
    // Act
    // Assert
    assert_source_ownership(source, false);
}

#[test]
fn should_use_the_outer_query_owner_for_a_nested_reference_constructor_scan() {
    // Arrange
    let (cassie, session) = observer::fixture();
    let controls = controls();
    let functions = HashMap::new();
    let descriptors =
        crate::executor::filter::graph_functions::descriptors_of(Some(&session)).expect("types");
    let key = KeyPart::Int(1).to_value(&descriptors).expect("key");
    let params = [Value::Array(
        SqlArray::from_elements(vec![key]).expect("key array"),
    )];
    let constructor = FunctionCall {
        name: "cassie_graph.vertex_ref".to_owned(),
        args: vec![
            Expr::StringLiteral("g".to_owned()),
            Expr::StringLiteral("n".to_owned()),
            Expr::Param(0),
        ],
        ..FunctionCall::default()
    };
    let function = FunctionCall {
        name: "cassie_graph.key_parts".to_owned(),
        args: vec![Expr::Function(Box::new(constructor))],
        ..FunctionCall::default()
    };
    let env = SourceExecutionEnv {
        cassie: &cassie,
        session: Some(&session),
        user_functions: &functions,
        params: &params,
        parameter_type_oids: &[],
        controls: &controls,
    };
    let _observation = observer::start();
    observer::expect_owner(&controls);

    // Act
    let result = execute(&env, &function.name, &function, None, false).expect("nested constructor");

    // Assert
    assert_eq!(result.0.iter().map(Vec::len).sum::<usize>(), 1);
    let owners = observer::events()
        .into_iter()
        .filter(|(boundary, _)| *boundary == "constructor_scan_owner")
        .map(|(_, same_owner)| same_owner)
        .collect::<Vec<_>>();
    assert_eq!(
        owners,
        vec![1],
        "existence reads must share the active outer tracker"
    );
}

#[test]
fn should_admit_live_prefix_costs_and_actual_aligned_operands_on_the_cost_owner() {
    // Arrange
    let (cassie, session) = observer::fixture();
    let descriptors =
        crate::executor::filter::graph_functions::descriptors_of(Some(&session)).expect("types");
    let vertex = reference();
    let edge = ElementRef {
        kind: ElementKind::Edge,
        mapping_id: uuid::Uuid::from_u128(4),
        ..vertex.clone()
    };
    let path = Path {
        vertices: vec![vertex.clone(), vertex.clone(), vertex],
        edges: vec![edge.clone(), edge],
        directions: vec![Direction::Forward, Direction::Forward],
    }
    .to_value(&descriptors)
    .expect("two-edge native path");
    let wide =
        crate::types::extended::numeric::parse_text(&"1".repeat(2_048)).expect("finite wide cost");
    let costs = Value::Array(
        SqlArray::from_elements(vec![
            Value::Typed(TypedValue::Numeric(wide.clone())),
            Value::Typed(TypedValue::Numeric(wide)),
        ])
        .expect("two costs"),
    );
    let functions = HashMap::new();
    let controls = controls();
    let env = SourceExecutionEnv {
        cassie: &cassie,
        session: Some(&session),
        user_functions: &functions,
        params: &[],
        parameter_type_oids: &[],
        controls: &controls,
    };
    let mut output = controls.reserve_query_memory(0).expect("output owner");
    let _observation = observer::start();
    let _alignment = math::observe_add_alignment(|bytes| {
        observer::record("aligned_graph_prefix_costs", bytes);
    });

    // Act
    let rows = step_rows(&env, &path, Some(&costs), &mut output).expect("finite prefix costs");
    let events = observer::events();

    // Assert
    assert_eq!(rows.len(), 3);
    let mut admitted = None;
    let mut live = None;
    let mut additions = 0;
    for (boundary, bytes) in events {
        match boundary {
            "admitted_graph_prefix_costs" => admitted = Some(bytes),
            "live_graph_prefix_costs" => live = Some(bytes),
            "aligned_graph_prefix_costs" if live.is_some() => {
                let required = live.expect("live prefix boundary") + bytes;
                let charged = admitted.expect("specific cost owner");
                assert!(
                    charged >= required,
                    "prefix owner admits {charged}, but live payload alone requires {required}"
                );
                additions += 1;
            }
            _ => {}
        }
    }
    assert_eq!(additions, 2);
}
