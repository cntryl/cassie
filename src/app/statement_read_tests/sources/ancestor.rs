//! Nested ancestor reads share the captured statement across an actual writer commit.
use super::*;

#[test]
fn should_preserve_nested_ancestor_reads_after_a_committed_writer() {
    // Arrange
    for output in [true, false] {
        let fixture = Fixture::new();
        let reader = fixture.cassie.create_session("reader", None);
        execute(
            &fixture.cassie,
            &reader,
            "CREATE TABLE read_membership(id INT PRIMARY KEY)",
        );
        execute(
            &fixture.cassie,
            &reader,
            "INSERT INTO read_membership VALUES (1)",
        );
        let cassie = Arc::clone(&fixture.cassie);
        let writer = cassie.create_session("writer", None);
        let committed = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&committed);
        let hook = crate::executor::JoinReadProbe::install(move || {
            execute(&cassie, &writer, "BEGIN");
            execute(&cassie, &writer, "DELETE FROM read_membership WHERE id=1");
            execute(&cassie, &writer, "INSERT INTO read_membership VALUES (2)");
            execute(&cassie, &writer, "COMMIT");
            observed.store(true, Ordering::SeqCst);
        });
        let predicate="EXISTS(SELECT 1 FROM read_left m WHERE m.id=l.id AND EXISTS(SELECT 1 FROM read_membership i WHERE i.id=l.id))";
        let sql = if output {
            format!("SELECT l.id,{predicate} AS matched FROM read_left l JOIN read_right r ON l.id=r.id ORDER BY l.id")
        } else {
            format!("SELECT l.id FROM read_left l JOIN read_right r ON l.id=r.id WHERE {predicate} ORDER BY l.id")
        };
        // Act
        let captured = execute(&fixture.cassie, &reader, &sql);
        drop(hook);
        let fresh = execute(&fixture.cassie, &reader, &sql);
        // Assert
        assert!(
            committed.load(Ordering::SeqCst),
            "writer committed after outer left acquisition"
        );
        let (expected_captured, expected_fresh) = if output {
            (
                vec![
                    vec![Value::Int64(1), Value::Bool(true)],
                    vec![Value::Int64(2), Value::Bool(false)],
                ],
                vec![
                    vec![Value::Int64(1), Value::Bool(false)],
                    vec![Value::Int64(2), Value::Bool(true)],
                ],
            )
        } else {
            (vec![vec![Value::Int64(1)]], vec![vec![Value::Int64(2)]])
        };
        assert_eq!(captured, expected_captured);
        assert_eq!(fresh, expected_fresh);
    }
}
