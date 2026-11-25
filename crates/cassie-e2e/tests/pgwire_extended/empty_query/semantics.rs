use std::time::Duration;

use super::support_pgwire as support;

const ANSWER_LIMIT: Duration = Duration::from_secs(10);

/// Sends each batch in turn on one connection and returns the tags of the
/// frames answering each batch through its `ReadyForQuery`.
fn run_batches(label: &str, batches: Vec<Vec<Vec<u8>>>) -> Vec<Vec<u8>> {
    let path = support::data_dir(label);
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
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
            let frames = support::read_frames_until_ready_within(&mut reader, ANSWER_LIMIT).await;
            answers.push(frames.into_iter().map(|(tag, _)| tag).collect());
        }
        server.stop().await;
        answers
    });
    let _ = std::fs::remove_dir_all(path);
    answers
}

#[test]
fn should_answer_empty_query_response_given_simple_query_without_statements() {
    // Arrange
    let inputs = ["", "   ", ";", "-- just a comment", "/* block */ ;"];
    let batches = inputs
        .iter()
        .map(|sql| vec![support::simple_query_frame(sql)])
        .collect();

    // Act
    let answers = run_batches("empty-simple-query", batches);

    // Assert
    assert_eq!(answers, vec![b"IZ".to_vec(); inputs.len()]);
}

#[test]
fn should_answer_empty_query_response_given_extended_empty_statement() {
    // Arrange
    let batches = vec![
        vec![
            support::parse_frame("empty_stmt", ""),
            support::describe_statement_frame("empty_stmt"),
            support::bind_frame("empty_portal", "empty_stmt", &[]),
            support::describe_portal_frame("empty_portal"),
            support::execute_frame("empty_portal"),
            support::sync_frame(),
        ],
        vec![support::simple_query_frame("SELECT 1")],
    ];

    // Act
    let answers = run_batches("empty-extended-query", batches);

    // Assert
    assert_eq!(answers[0], b"1tn2nIZ".to_vec());
    assert_eq!(answers[1], b"TDCZ".to_vec());
}
