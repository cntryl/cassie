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
}
