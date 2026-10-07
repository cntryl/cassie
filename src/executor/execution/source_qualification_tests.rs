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
