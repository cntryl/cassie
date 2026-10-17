//! Statement identity survives rebinding and suspension without becoming cached data.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

fn assert_text(frames: &fixture::Frames, sql: &str) {
    fixture::assert_success(frames);
    assert_eq!(wire::data_rows(frames), vec![vec![Some(sql.to_owned())]]);
}

#[test]
fn should_preserve_parse_text_given_rebinding_when_other_statements_execute_between_portals() {
    // Arrange
    let sql = "/* prepared origin */ SELECT current_query() AS output WHERE $1::INT = 1";
    let unrelated = "/* another origin */ SELECT current_query() AS output";
    let mut cycles = vec![vec![
        wire::parse_frame_with_types("s", sql, &[23]),
        wire::sync_frame(),
    ]];
    for format in [0, 1] {
        cycles.push(vec![wire::simple_query_frame(unrelated)]);
        cycles.push(vec![
            wire::bind_frame_with_formats("p", "s", &[0], &[Some(b"1")], &[format]),
            wire::describe_portal_frame("p"),
            wire::execute_frame("p"),
            wire::sync_frame(),
        ]);
    }
    // Act
    let batches = fixture::run_cycles("query-rebind", &[], cycles);
    // Assert
    fixture::assert_success(&batches[0]);
    for (index, cycle) in batches[1..].as_chunks::<2>().0.iter().enumerate() {
        assert_text(&cycle[0], unrelated);
        assert_text(&cycle[1], sql);
        let description = cycle[1]
            .iter()
            .find(|f| f.0 == b'T')
            .expect("portal description");
        let columns = wire::parse_row_description(&description.1);
        assert_eq!(
            (
                columns[0].type_oid,
                columns[0].type_size,
                columns[0].type_mod,
                columns[0].format_code
            ),
            (25, -1, -1, i16::try_from(index).expect("format"))
        );
    }
}

#[test]
fn should_preserve_parse_text_given_suspended_portal_when_a_different_query_executes_between_pages()
{
    // Arrange
    let sql = "/* portal origin */ SELECT current_query() AS output FROM (VALUES (1),(2),(3)) q(n)";
    let other = "/* intervening origin */ SELECT current_query() AS output";
    let cycles = vec![
        vec![wire::simple_query_frame("BEGIN")],
        vec![
            wire::parse_frame("s", sql),
            wire::bind_frame("p", "s", &[]),
            wire::describe_portal_frame("p"),
            wire::execute_limited_frame("p", 1),
            wire::sync_frame(),
        ],
        vec![wire::simple_query_frame(other)],
        vec![wire::execute_limited_frame("p", 1), wire::sync_frame()],
        vec![wire::execute_limited_frame("p", 1), wire::sync_frame()],
        vec![wire::simple_query_frame("ROLLBACK")],
    ];
    // Act
    let batches = fixture::run_cycles("query-paging", &[], cycles);
    // Assert
    assert_eq!(batches.len(), 6);
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[5]), None);
    for index in [1, 3, 4] {
        assert_eq!(wire::error_code(&batches[index]), None);
        assert_eq!(
            wire::data_rows(&batches[index]),
            vec![vec![Some(sql.to_owned())]]
        );
    }
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some(other.to_owned())]]
    );
    assert_eq!(batches[1].iter().filter(|f| f.0 == b's').count(), 1);
}

#[test]
fn should_refresh_statement_text_given_same_ast_with_different_comments_when_execution_cache_is_enabled(
) {
    // Arrange
    let queries = [
        "/* first */ SELECT current_query() AS output",
        "/* second */ SELECT current_query() AS output",
        "/* first */ SELECT current_query() AS output",
    ];
    // Act
    let batches = fixture::run_cycles(
        "query-cache",
        &[],
        queries
            .iter()
            .map(|sql| vec![wire::simple_query_frame(sql)])
            .collect(),
    );
    // Assert
    for (frames, sql) in batches.iter().zip(queries) {
        assert_text(frames, sql);
    }
}

#[test]
fn should_return_outer_statement_given_a_stored_view_when_current_query_is_evaluated() {
    // Arrange
    let queries = [
        "/* first view */ SELECT output FROM query_text_view",
        "/* second view */ SELECT output FROM query_text_view",
    ];
    // Act
    let batches = fixture::run_cycles(
        "query-view",
        &[(
            "CREATE VIEW query_text_view AS SELECT pg_catalog.current_query() AS output",
            vec![],
        )],
        queries
            .iter()
            .map(|sql| vec![wire::simple_query_frame(sql)])
            .collect(),
    );
    // Assert
    for (frames, sql) in batches.iter().zip(queries) {
        assert_text(frames, sql);
    }
}

#[test]
fn should_recover_statement_text_given_an_execution_error_when_the_next_query_runs() {
    // Arrange
    let sql = "/* after failure */ SELECT current_query() AS output";
    // Act
    let batches = fixture::run_cycles(
        "query-recovery",
        &[],
        vec![
            vec![wire::simple_query_frame("SELECT 1 / 0")],
            vec![wire::simple_query_frame(sql)],
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22012"));
    assert_text(&batches[1], sql);
}
