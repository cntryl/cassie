use super::super::super::{compiler, matcher};
use super::{search, Directory, Input, Regex, Walk};
use crate::{
    app::CassieError,
    config::CassieRuntimeLimits,
    executor::filter::QueryError,
    runtime::{QueryCancellationHandle, QueryExecutionControls},
};

fn controls(bytes: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: bytes,
            ..CassieRuntimeLimits::default()
        },
        std::time::Instant::now(),
    )
}

#[test]
fn should_admit_regex_directory_replacement_before_allocating() {
    // Arrange
    let below = controls(39);
    let above = controls(40);
    let mut rejected = Directory::with_capacity(1, Some(&below)).expect("initial eight bytes");
    rejected.push(1_u64, Some(&below)).expect("first value");
    let mut accepted = Directory::with_capacity(1, Some(&above)).expect("initial eight bytes");
    accepted.push(1_u64, Some(&above)).expect("first value");

    // Act
    let failure = rejected.push(2, Some(&below));
    let success = accepted.push(2, Some(&above));
    let below_retained = below.current_query_memory_bytes();
    let above_retained = above.current_query_memory_bytes();
    drop(rejected);
    drop(accepted);

    // Assert
    assert!(matches!(
        failure,
        Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
    ));
    assert!(success.is_ok());
    assert_eq!(below_retained, 8);
    assert_eq!(above_retained, 32);
    assert_eq!(above.peak_query_memory_bytes(), 40);
    assert_eq!(below.current_query_memory_bytes(), 0);
    assert_eq!(above.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_constraint_search_backing_between_searches() {
    // Arrange
    let controls = controls(8 * 1024 * 1024);
    let regex =
        compiler::compile(r"(a|aa)+(?=b)b", "", Some(&controls)).expect("constraint program");
    let retained = controls.current_query_memory_bytes();

    // Act
    let matched = regex.is_match("aaaab", Some(&controls));
    let after_match = controls.current_query_memory_bytes();
    let missed = regex.is_match("aaaac", Some(&controls));
    let after_miss = controls.current_query_memory_bytes();
    drop(regex);

    // Assert
    assert!(matched.expect("bounded matching succeeds"));
    assert!(!missed.expect("bounded miss succeeds"));
    assert!(retained > 0);
    assert_eq!(after_match, retained);
    assert_eq!(after_miss, retained);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_constraint_search_state_after_failed_admission() {
    // Arrange
    let controls = controls(8 * 1024 * 1024);
    let regex =
        compiler::compile(r"(a|aa)+(?=b)b", "", Some(&controls)).expect("constraint program");
    let retained = controls.current_query_memory_bytes();
    let blocker = controls
        .reserve_query_memory(8 * 1024 * 1024 - retained)
        .expect("consume remaining budget");

    // Act
    let result = regex.is_match("aaaab", Some(&controls));
    let after_error = controls.current_query_memory_bytes();
    drop(blocker);
    let recovered = regex.is_match("aaaab", Some(&controls));
    drop(regex);

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
    ));
    assert_eq!(after_error, 8 * 1024 * 1024);
    assert!(recovered.expect("same program recovers after admission failure"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_constraint_work_after_interpreter_progress() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        std::time::Instant::now(),
        cancellation.clone(),
    );
    let program = compiler::compile(r"(?<=a+)b", "", Some(&controls)).expect("lookbehind program");
    let matcher::Regex::Constraint(regex) = &program else {
        panic!("hard pattern uses owned interpreter")
    };
    let retained = controls.current_query_memory_bytes();
    let mut steps = 0;
    let mut check = || {
        steps += 1;
        if steps == 32 {
            cancellation.cancel();
        }
        controls.check_interruption().map_err(QueryError::from)
    };

    // Act
    let result = search(regex, &Input::new("aaaab"), Some(&controls), &mut check);
    let after_error = controls.current_query_memory_bytes();
    drop(program);

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::QueryCancelled))
    ));
    assert_eq!(steps, 32);
    assert_eq!(after_error, retained);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_bound_constraint_execution_when_no_deadline_is_configured() {
    // Arrange
    let regex = Regex {
        nodes: Directory::new(),
        groups: 1,
        _syntax_memory: None,
    };
    let mut check = || Ok(());
    let mut walk = Walk {
        regex: &regex,
        text: "",
        controls: None,
        remaining: 0,
        check: &mut check,
    };

    // Act
    let result = walk.tick();

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
    ));
}

#[test]
fn should_retain_constraint_capture_admission_after_program_drop() {
    // Arrange
    let controls = controls(8 * 1024 * 1024);
    let regex = compiler::compile(r"(ab)\1", "", Some(&controls)).expect("backreference program");
    let mut captures = regex
        .create_captures(Some(&controls))
        .expect("admitted output buffer");

    // Act
    regex
        .search_captures(&Input::new("abab"), &mut captures, Some(&controls))
        .expect("capture search");
    drop(regex);
    let retained = controls.current_query_memory_bytes();
    let group = captures.get_group(1);
    drop(captures);

    // Assert
    assert_eq!(group, Some(regex_automata::Span::from(0..2)));
    assert!(retained > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_check_interruption_while_skipping_unrelated_priority_tags() {
    // Arrange
    let regex = Regex {
        nodes: Directory::new(),
        groups: 1,
        _syntax_memory: None,
    };
    let tags = [super::Tag {
        node: 1,
        span: regex_automata::Span::from(0..1),
    }; 8];
    let mut checks = 0;
    let mut check = || {
        checks += 1;
        if checks == 4 {
            Err(CassieError::QueryCancelled.into())
        } else {
            Ok(())
        }
    };
    let mut walk = Walk {
        regex: &regex,
        text: "a",
        controls: None,
        remaining: 100,
        check: &mut check,
    };
    let mut offset = 0;

    // Act
    let result = walk.next_tag(&tags, 0, &mut offset);

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::QueryCancelled))
    ));
    assert_eq!(offset, 3);
}

#[test]
fn should_reject_large_numeric_targets_without_target_sized_allocation() {
    // Arrange
    let controls = controls(128 * 1024);

    // Act
    let result = compiler::compile(r"(a)\999999999", "", Some(&controls));

    // Assert
    assert!(
        matches!(result, Err(QueryError::Cassie(CassieError::Diagnostic(ref diagnostic))) if diagnostic.sql_state == "2201B")
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_constraint_parser_recursion_above_its_finite_limit() {
    // Arrange
    let controls = controls(1024 * 1024);
    let pattern = format!("{}a{}(?=)", "(".repeat(65), ")".repeat(65));

    // Act
    let result = compiler::compile(&pattern, "", Some(&controls));

    // Assert
    assert!(
        matches!(result, Err(QueryError::Cassie(CassieError::Diagnostic(ref diagnostic))) if diagnostic.sql_state == "2201B")
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
