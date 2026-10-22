use std::time::Duration;

use super::support_pgwire as support;

const ANSWER_LIMIT: Duration = Duration::from_secs(10);

type Frames = Vec<(u8, Vec<u8>)>;

/// Starts a server over a database prepared by `setup`, sends each batch
/// of frames in turn on one connection, and returns the frames answering
/// each batch through its `ReadyForQuery`.
fn run_batches(label: &str, setup: &[&str], batches: Vec<Vec<Vec<u8>>>) -> Vec<Frames> {
    let path = support::data_dir(label);
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in setup {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("setup sql");
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let answers = runtime.block_on(async {
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (read_half, mut writer) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut writer).await;
        let mut answers = Vec::new();
        for batch in batches {
            support::write_frames(&mut writer, batch).await;
            answers.push(support::read_frames_until_ready_within(&mut reader, ANSWER_LIMIT).await);
        }
        server.stop().await;
        answers
    });
    let _ = std::fs::remove_dir_all(path);
    answers
}

fn data_row_widths(frames: &Frames) -> Vec<usize> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'D')
        .map(|(_, payload)| usize::from(u16::from_be_bytes([payload[0], payload[1]])))
        .collect()
}

#[test]
fn should_expand_wildcard_in_schema_order_given_row_limited_portal_with_explicit_column() {
    // Arrange
    let setup = [
        "CREATE TABLE rl_docs (title TEXT, score FLOAT)",
        "INSERT INTO rl_docs (title, score) VALUES ('c', 3.5)",
    ];

    // Act
    let answers = run_batches(
        "row-limited-wildcard-order",
        &setup,
        vec![vec![
            support::parse_frame("s", "SELECT *, score FROM rl_docs"),
            support::bind_frame("p", "s", &[]),
            support::describe_portal_frame("p"),
            support::execute_limited_frame("p", 2),
            support::sync_frame(),
        ]],
    );

    // Assert
    let frames = &answers[0];
    assert_eq!(
        support::row_description_names(frames),
        vec!["id", "title", "score", "score"]
    );
    let rows = support::data_rows(frames);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0][1..].to_vec(),
        vec![
            Some("c".to_string()),
            Some("3.5".to_string()),
            Some("3.5".to_string()),
        ]
    );
}

#[test]
fn should_keep_row_width_given_binary_row_limited_portal_with_repeated_column() {
    // Arrange
    let setup = [
        "CREATE TABLE rl_onecol (title TEXT)",
        "INSERT INTO rl_onecol (title) VALUES ('x'), ('y')",
    ];

    // Act
    let answers = run_batches(
        "row-limited-binary-width",
        &setup,
        vec![
            vec![
                support::parse_frame("s", "SELECT *, TITLE FROM rl_onecol"),
                support::bind_frame_with_formats("p", "s", &[], &[], &[1]),
                support::describe_portal_frame("p"),
                support::execute_limited_frame("p", 1),
                support::sync_frame(),
            ],
            vec![support::simple_query_frame("SELECT 1")],
        ],
    );

    // Assert
    let described = support::row_description_names(&answers[0]).len();
    assert_eq!(data_row_widths(&answers[0]), vec![described]);
    assert_eq!(answers[1].last().map(|frame| frame.0), Some(b'Z'));
    assert_eq!(support::data_rows(&answers[1]).len(), 1);
}

#[test]
fn should_send_repeated_column_value_given_text_row_limited_portal() {
    // Arrange
    let setup = [
        "CREATE TABLE rl_textcol (title TEXT)",
        "INSERT INTO rl_textcol (title) VALUES ('x')",
    ];

    // Act
    let answers = run_batches(
        "row-limited-text-width",
        &setup,
        vec![vec![
            support::parse_frame("s", "SELECT *, TITLE FROM rl_textcol"),
            support::bind_frame("p", "s", &[]),
            support::describe_portal_frame("p"),
            support::execute_limited_frame("p", 1),
            support::sync_frame(),
        ]],
    );

    // Assert
    let frames = &answers[0];
    assert_eq!(support::row_description_names(frames).len(), 3);
    let rows = support::data_rows(frames);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0][1..].to_vec(),
        vec![Some("x".to_string()), Some("x".to_string())]
    );
}

fn command_tags(frames: &Frames) -> Vec<String> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'C')
        .map(|(_, payload)| {
            String::from_utf8_lossy(payload)
                .trim_end_matches('\0')
                .to_string()
        })
        .collect()
}

/// Batches that page `sql` two rows at a time and then drain the rest.
fn resumed_batches(sql: &str) -> Vec<Vec<Vec<u8>>> {
    vec![
        vec![support::simple_query_frame("BEGIN")],
        vec![
            support::parse_frame("", sql),
            support::bind_frame("p", "", &[]),
            support::execute_limited_frame("p", 2),
            support::sync_frame(),
        ],
        vec![
            support::execute_limited_frame("p", 100),
            support::sync_frame(),
        ],
    ]
}

const TITLE_SETUP: [&str; 2] = [
    "CREATE TABLE rl_titles (title TEXT)",
    "INSERT INTO rl_titles (title) VALUES ('a'), ('b'), ('c'), ('d')",
];

#[test]
fn should_report_rows_of_final_execute_given_resumed_streaming_portal() {
    // Arrange
    let batches = resumed_batches("SELECT title FROM rl_titles");

    // Act
    let answers = run_batches("row-limited-tag-streamed", &TITLE_SETUP, batches);

    // Assert
    assert_eq!(command_tags(&answers[2]), vec!["SELECT 2".to_string()]);
}

#[test]
fn should_report_rows_of_final_execute_given_resumed_materialized_portal() {
    // Arrange
    let batches = resumed_batches("SELECT title FROM rl_titles ORDER BY title");

    // Act
    let answers = run_batches("row-limited-tag-materialized", &TITLE_SETUP, batches);

    // Assert
    assert_eq!(command_tags(&answers[2]), vec!["SELECT 2".to_string()]);
}

#[test]
fn should_report_all_rows_given_unlimited_portal_execute() {
    // Arrange
    let batches = vec![vec![
        support::parse_frame("", "SELECT title FROM rl_titles ORDER BY title"),
        support::bind_frame("", "", &[]),
        support::execute_frame(""),
        support::sync_frame(),
    ]];

    // Act
    let answers = run_batches("unlimited-portal-tag", &TITLE_SETUP, batches);

    // Assert
    assert_eq!(command_tags(&answers[0]), vec!["SELECT 4".to_string()]);
}

#[test]
fn should_report_row_count_given_simple_query_select() {
    // Arrange
    let batches = vec![
        vec![support::simple_query_frame("SELECT title FROM rl_titles")],
        vec![support::simple_query_frame(
            "SELECT title FROM rl_titles WHERE title = 'none'",
        )],
    ];

    // Act
    let answers = run_batches("simple-select-tag", &TITLE_SETUP, batches);

    // Assert
    assert_eq!(command_tags(&answers[0]), vec!["SELECT 4".to_string()]);
    assert_eq!(command_tags(&answers[1]), vec!["SELECT 0".to_string()]);
}
