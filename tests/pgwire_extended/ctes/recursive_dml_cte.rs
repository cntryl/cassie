//! PostgreSQL 18 rejection of recursive data-modifying CTEs tracked by #834.

use super::super::{support_pgwire as wire, support_type_metadata_contract as fixture};

fn run_recursive_dml_probe(label: &str, relation: &str, mutation: &str) -> Vec<fixture::Frames> {
    let create = format!("CREATE TABLE {relation} (id INT PRIMARY KEY, payload INT)");
    let seed = format!("INSERT INTO {relation} VALUES (1, 0)");
    let observer = format!("SELECT id, payload FROM {relation} ORDER BY id");
    fixture::run_cycles(
        label,
        &[(create.as_str(), Vec::new()), (seed.as_str(), Vec::new())],
        vec![
            vec![wire::simple_query_frame(mutation)],
            vec![wire::simple_query_frame(&observer)],
        ],
    )
}

#[test]
fn should_reject_recursive_update_ctes_with_postgres_invalid_recursion_sqlstate() {
    // Arrange
    let relation = "issue_834_recursive_update_target";
    let mutation = "WITH RECURSIVE changed(id) AS ( \
         UPDATE issue_834_recursive_update_target SET payload = payload + 1 \
         WHERE id IN (SELECT id FROM changed) RETURNING id \
     ) SELECT id FROM changed";

    // Act
    let batches = run_recursive_dml_probe("issue-834-recursive-update-cte", relation, mutation);

    // Assert
    assert_recursive_dml_rejected(&batches);
}

#[test]
fn should_reject_recursive_delete_ctes_with_postgres_invalid_recursion_sqlstate() {
    // Arrange
    let relation = "issue_834_recursive_delete_target";
    let mutation = "WITH RECURSIVE removed(id) AS ( \
         DELETE FROM issue_834_recursive_delete_target \
         WHERE id IN (SELECT id FROM removed) RETURNING id \
     ) SELECT id FROM removed";

    // Act
    let batches = run_recursive_dml_probe("issue-834-recursive-delete-cte", relation, mutation);

    // Assert
    assert_recursive_dml_rejected(&batches);
}

#[test]
fn should_reject_recursive_insert_ctes_with_postgres_invalid_recursion_sqlstate() {
    // Arrange
    let relation = "issue_834_recursive_insert_target";
    let mutation = "WITH RECURSIVE inserted(id) AS ( \
         INSERT INTO issue_834_recursive_insert_target \
         SELECT id + 1, payload FROM issue_834_recursive_insert_target \
         WHERE id IN (SELECT id FROM inserted) RETURNING id \
     ) SELECT id FROM inserted";

    // Act
    let batches = run_recursive_dml_probe("issue-834-recursive-insert-cte", relation, mutation);

    // Assert
    assert_recursive_dml_rejected(&batches);
}

fn assert_recursive_dml_rejected(batches: &[fixture::Frames]) {
    let rejected = &batches[0];
    assert_eq!(wire::error_code(rejected).as_deref(), Some("42P19"));
    assert_eq!(wire::data_rows(rejected), Vec::<Vec<Option<String>>>::new());
    assert_eq!(rejected.iter().filter(|(tag, _)| *tag == b'C').count(), 0);
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("1".to_owned()), Some("0".to_owned())]],
        "PostgreSQL rejects a recursive data-modifying CTE before changing its target"
    );
}
