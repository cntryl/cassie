use super::*;
use std::mem::size_of;
use std::sync::Arc;
use std::time::Instant;

fn with_fixture(run: impl FnOnce(&Cassie)) {
    let path =
        std::env::temp_dir().join(format!("cassie-857-qualification-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(&path).expect("fixture");
    run(&cassie);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict fixture cleanup");
}

#[test]
fn should_retain_fresh_derived_qualification_backing_with_parent_roots() {
    // Arrange
    with_fixture(|cassie| {
        let limits = crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 1024 * 1024,
            ..crate::config::CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let parent = Arc::new(
            controls
                .reserve_query_memory(65_536)
                .expect("source parent"),
        );
        let row = BatchRow::new(vec![("n".into(), Value::Int64(1))])
            .with_query_memory(Some(Arc::clone(&parent)))
            .retain_operator_memory(&controls, Arc::clone(&parent))
            .expect("operator root");
        let before = controls.current_query_memory_bytes();
        let functions = HashMap::new();
        let env = SourceExecutionEnv {
            cassie,
            session: None,
            user_functions: &functions,
            params: &[],
            controls: &controls,
        };
        let qualifier = "q".repeat(4096);
        // Act
        let (output, _) =
            finalize_source_batches(&env, vec![vec![row]], Vec::new(), true, &qualifier)
                .expect("qualified derived output");
        // Assert
        assert!(
            controls.current_query_memory_bytes() > before + qualifier.len(),
            "new aliases and eager lookup must retain independent charge"
        );
        assert_eq!(
            output[0][0].get(&format!("{qualifier}.n")),
            Some(&Value::Int64(1))
        );
        drop(parent);
        assert!(controls.current_query_memory_bytes() > 65_536);
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_deny_derived_qualification_before_constructing_long_aliases() {
    // Arrange
    with_fixture(|cassie| {
        let limits = crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 65_536 + 1024,
            ..crate::config::CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let parent = Arc::new(
            controls
                .reserve_query_memory(65_536)
                .expect("source parent"),
        );
        let row = BatchRow::new(vec![("n".into(), Value::Int64(1))])
            .with_query_memory(Some(Arc::clone(&parent)))
            .retain_operator_memory(&controls, Arc::clone(&parent))
            .expect("operator root");
        let functions = HashMap::new();
        let env = SourceExecutionEnv {
            cassie,
            session: None,
            user_functions: &functions,
            params: &[],
            controls: &controls,
        };
        // Act
        let result =
            finalize_source_batches(&env, vec![vec![row]], Vec::new(), true, &"q".repeat(4096));
        // Assert
        assert!(
            matches!(
                result.map_err(crate::app::CassieError::from),
                Err(crate::app::CassieError::ResourceLimit(_))
            ),
            "old roots cannot admit newly constructed aliases"
        );
        assert_eq!(controls.current_query_memory_bytes(), 65_536);
        drop(parent);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_bound_actual_non_power_of_two_alias_growth() {
    // Arrange
    let aliases = (0..10)
        .map(|index| (format!("a{index}"), 0))
        .collect::<Vec<_>>();
    assert_eq!(aliases.capacity(), 10);
    let row = BatchRow::with_aliases(vec![("n".into(), Value::Int64(1))], aliases);
    let admitted =
        super::source_collection::row_qualification_bytes("q", &row).expect("qualification bound");
    // Act
    let qualified = qualify_row(row, "q");
    let (entries, aliases) = qualified.into_parts();
    // Assert
    assert_eq!(aliases.len(), 11);
    assert_eq!(aliases.capacity(), 20);
    let names = entries.iter().map(|(name, _)| name.len()).sum::<usize>()
        + aliases.iter().map(|(name, _)| name.len()).sum::<usize>();
    let lookup =
        crate::executor::retained_memory::lookup_bytes(entries.len() + aliases.len(), names)
            .expect("eager lookup authority");
    let minimum = aliases.capacity() * size_of::<(String, usize)>()
        + lookup
        + aliases.last().expect("fresh alias").0.capacity();
    assert!(
        admitted >= minimum,
        "actual capacity20 must be bounded, not rounded16: admitted={admitted} minimum={minimum}"
    );
}

#[test]
fn should_retain_cte_array_source_descriptor_replacement_until_output_drop() {
    // Arrange
    with_fixture(|cassie| {
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            Instant::now(),
        );
        let parent = Arc::new(controls.reserve_query_memory(65_536).expect("source owner"));
        let context = CteContext::unleased(HashMap::from([(
            "c".into(),
            super::super::cte::CteRelation {
                rows: vec![],
                fields: vec![crate::types::FieldSchema {
                    name: "n".into(),
                    data_type: crate::types::DataType::Array(Box::new(
                        crate::types::DataType::Text,
                    )),
                    nullable: true,
                }],
                _rows_memory: None,
                _fields_memory: None,
            },
        )]));
        let functions = HashMap::new();
        let env = SourceExecutionEnv {
            cassie,
            session: None,
            user_functions: &functions,
            params: &[],
            controls: &controls,
        };
        let mut batches = vec![vec![BatchRow::new(vec![("n".into(), Value::Null)])
            .with_query_memory(Some(Arc::clone(&parent)))]];
        // Act
        source_shape::attach_types(&env, &QuerySource::Cte("c".into()), &context, &mut batches)
            .expect("existing ARRAY descriptor");
        // Assert
        assert!(
            controls.current_query_memory_bytes() > 65_536,
            "new shared descriptors must retain replacement charge"
        );
        let types = batches[0][0]
            .shared_data_types()
            .expect("shared descriptor");
        let root = batches[0][0].operator_memory().expect("descriptor root");
        let nodes = 2 * (std::mem::size_of_val(root.as_ref()) + 2 * size_of::<usize>());
        let heap = crate::executor::retained_memory::data_type_clone_bytes(&types[0])
            .expect("nested descriptor heap");
        let minimum = nodes
            + size_of::<Vec<crate::types::DataType>>()
            + 2 * size_of::<usize>()
            + types.capacity() * size_of::<crate::types::DataType>()
            + heap
            + size_of::<crate::runtime::QueryMemoryReservation>()
            + 2 * size_of::<usize>();
        assert!(
            controls.current_query_memory_bytes() - 65_536 >= minimum,
            "retained type capacity{} must be charged: delta{} minimum{minimum}",
            types.capacity(),
            controls.current_query_memory_bytes() - 65_536
        );
        drop(root);
        drop(types);

        assert_eq!(
            batches[0][0].data_types()[0],
            crate::types::DataType::Array(Box::new(crate::types::DataType::Text))
        );
        drop(parent);
        assert!(controls.current_query_memory_bytes() > 65_536);
        drop(batches);
        drop(context);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}
