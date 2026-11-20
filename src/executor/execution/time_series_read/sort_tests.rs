use super::sort_time_series_documents;
use crate::midge::adapter::DocumentRef;

fn document(id: &str, at: &str) -> DocumentRef {
    DocumentRef {
        id: id.to_owned(),
        payload: serde_json::json!({"event_at": at}),
    }
}

#[test]
fn should_order_candidates_by_timestamp_then_row_id() {
    // Arrange
    let mut documents = vec![
        document("b", "2026-01-02T00:00:00.000000Z"),
        document("c", "2026-01-01T00:00:00.000000Z"),
        document("a", "2026-01-02T00:00:00.000000Z"),
        document("d", "2026-01-01T12:00:00.000000Z"),
    ];

    // Act
    sort_time_series_documents(&mut documents, "event_at");

    // Assert
    let ids = documents
        .iter()
        .map(|document| document.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["c", "d", "a", "b"]);
}
