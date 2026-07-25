#[path = "support/query_evidence.rs"]
mod query_evidence;

#[test]
fn should_compare_seeded_indexed_pages_with_overlay() {
    // Arrange
    let fixture = query_evidence::SeededQueryFixture::new(37);

    // Act
    let pages = fixture.compare_indexed_pages_with_overlay();

    // Assert
    assert_eq!(pages.indexed, pages.row_baseline);
    assert_eq!(pages.indexed.len(), 4);
    assert!(pages.indexed.iter().all(|page| page.len() <= 3));
    assert!(pages.indexed[0].iter().any(|row| {
        row == &vec![
            cassie::types::Value::String("overlay".to_owned()),
            cassie::types::Value::Int64(100),
        ]
    }));
}
