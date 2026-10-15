//! Polymorphic function argument identity through the extended query protocol.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

const CREATE_POLYMORPHIC_ECHO: &str = "CREATE FUNCTION issue_904_echo_pair(anyelement, anyarray) \
    RETURNS anyelement LANGUAGE SQL IMMUTABLE AS 'SELECT $1'";

fn error_response_fields(frames: &[(u8, Vec<u8>)]) -> Vec<(char, String)> {
    frames
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .unwrap_or_default()
}

#[test]
fn should_preserve_polymorphic_identity_given_matching_anyelement_and_anyarray_arguments_when_a_statement_is_prepared(
) {
    // Arrange
    let statement = "issue_904_matching_polymorphic_arguments";
    let portal = "issue_904_matching_polymorphic_arguments_portal";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(CREATE_POLYMORPHIC_ECHO)]),
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT issue_904_echo_pair($1, $2)",
            &[23, 1007],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(portal, statement, &["42", "{1,2}"]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-904-matching-polymorphic-arguments", steps);

    // Assert
    assert_eq!(
        wire::error_code(&batches[0]),
        None,
        "function DDL: {:?}",
        error_response_fields(&batches[0])
    );
    assert_eq!(batches[1].first().map(|(tag, _)| *tag), Some(b'1'));
    let described = &batches[2];
    assert_eq!(
        wire::error_code(described),
        None,
        "Describe: {:?}",
        error_response_fields(described)
    );
    let parameter_description = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("polymorphic ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![23, 1007]
    );
    let row_description = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("polymorphic RowDescription");
    assert_eq!(
        wire::parse_row_description(&row_description.1)[0].type_oid,
        23
    );
    assert_eq!(
        wire::error_code(&batches[3]),
        None,
        "Execute: {:?}",
        error_response_fields(&batches[3])
    );
    assert_eq!(
        wire::parse_row_description(
            &batches[3]
                .iter()
                .find(|(tag, _)| *tag == b'T')
                .expect("polymorphic portal RowDescription")
                .1
        )[0]
        .type_oid,
        23
    );
    assert_eq!(
        wire::data_rows(&batches[3]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_infer_polymorphic_identity_given_unknown_arguments_and_typed_siblings_when_a_statement_is_prepared(
) {
    // Arrange
    let statement = "issue_904_inferred_polymorphic_element";
    let portal = "issue_904_inferred_polymorphic_element_portal";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(CREATE_POLYMORPHIC_ECHO)]),
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT issue_904_echo_pair($1, $2)",
            &[0, 1007],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(portal, statement, &["7", "{1,2}"]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-904-inferred-polymorphic-element", steps);

    // Assert
    assert_eq!(
        wire::error_code(&batches[0]),
        None,
        "function DDL: {:?}",
        error_response_fields(&batches[0])
    );
    assert_eq!(batches[1].first().map(|(tag, _)| *tag), Some(b'1'));
    let described = &batches[2];
    assert_eq!(
        wire::error_code(described),
        None,
        "Describe: {:?}",
        error_response_fields(described)
    );
    let parameter_description = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("inferred polymorphic ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![23, 1007]
    );
    let row_description = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("inferred polymorphic RowDescription");
    assert_eq!(
        wire::parse_row_description(&row_description.1)[0].type_oid,
        23
    );
    assert_eq!(
        wire::error_code(&batches[3]),
        None,
        "Execute: {:?}",
        error_response_fields(&batches[3])
    );
    assert_eq!(
        wire::data_rows(&batches[3]),
        vec![vec![Some("7".to_owned())]]
    );
}

#[test]
fn should_reject_unresolved_polymorphic_identity_given_only_unknown_arguments_when_a_statement_is_prepared(
) {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(CREATE_POLYMORPHIC_ECHO)]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT issue_904_echo_pair(NULL, NULL)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42 AS recovery")]),
    ];

    // Act
    let batches = run_script("issue-904-unresolved-polymorphic-arguments", steps);

    // Assert
    assert_eq!(
        wire::error_code(&batches[0]),
        None,
        "function DDL: {:?}",
        error_response_fields(&batches[0])
    );
    let rejected = &batches[1];
    assert_eq!(wire::error_code(rejected).as_deref(), Some("42804"));
    assert!(error_response_fields(rejected)
        .iter()
        .any(|(kind, message)| *kind == 'M'
            && message == "could not determine polymorphic type because input has type unknown"));
    assert!(!rejected
        .iter()
        .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_reject_conflicting_polymorphic_identity_given_incompatible_arguments_when_a_statement_is_prepared(
) {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(CREATE_POLYMORPHIC_ECHO)]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT issue_904_echo_pair(42, ARRAY['wrong']::TEXT[])",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42 AS recovery")]),
    ];

    // Act
    let batches = run_script("issue-904-conflicting-polymorphic-arguments", steps);

    // Assert
    assert_eq!(
        wire::error_code(&batches[0]),
        None,
        "function DDL: {:?}",
        error_response_fields(&batches[0])
    );
    let rejected = &batches[1];
    assert_eq!(wire::error_code(rejected).as_deref(), Some("42883"));
    assert!(error_response_fields(rejected)
        .iter()
        .any(|(kind, message)| *kind == 'M'
            && message
                .starts_with("function issue_904_echo_pair(integer, text[]) does not exist")));
    assert!(!rejected
        .iter()
        .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("42".to_owned())]]
    );
}
