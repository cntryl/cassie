use std::cell::RefCell;
use std::collections::HashMap;

use super::{eval_scalar, BinaryOp, Expr, RowAccess, ScalarValue, Value};

struct CountedRow {
    values: [(String, Value); 2],
    visits: RefCell<Vec<usize>>,
}

impl RowAccess for CountedRow {
    fn get(&self, name: &str) -> Option<&Value> {
        let index = self.values.iter().position(|(field, _)| field == name)?;
        self.visits.borrow_mut().push(index);
        Some(&self.values[index].1)
    }
    fn entries(&self) -> &[(String, Value)] {
        &self.values
    }
}

#[test]
fn should_evaluate_null_safe_operands_once_in_left_right_order() {
    // Arrange
    for op in [BinaryOp::IsDistinctFrom, BinaryOp::IsNotDistinctFrom] {
        for (left, right, equal) in [
            (Value::Null, Value::Null, true),
            (Value::Null, Value::Int64(7), false),
            (Value::Int64(7), Value::Null, false),
            (Value::Int64(7), Value::Int64(7), true),
        ] {
            let row = CountedRow {
                values: [("l".into(), left), ("r".into(), right)],
                visits: RefCell::new(Vec::new()),
            };
            let expr = Expr::Binary {
                left: Box::new(Expr::Column("l".into())),
                right: Box::new(Expr::Column("r".into())),
                op: op.clone(),
            };
            // Act
            let result = eval_scalar(&row, &expr, &[], None, &HashMap::new(), None, None)
                .expect("valid operands");
            // Assert
            let expected = if matches!(op, BinaryOp::IsNotDistinctFrom) {
                equal
            } else {
                !equal
            };
            assert!(matches!(result, ScalarValue::Bool(value) if value == expected));
            assert_eq!(*row.visits.borrow(), vec![0, 1]);
        }
    }
}

#[test]
fn should_preserve_expired_deadline_for_null_safe_predicates() {
    // Arrange
    let directory = QueryDirectory(std::env::temp_dir().join(format!(
        "cassie-null-safe-deadline-{}",
        uuid::Uuid::new_v4()
    )));
    std::fs::create_dir_all(&directory.0).expect("private query fixture directory");
    {
        let cassie =
            crate::app::Cassie::new_with_data_dir(directory.0.to_str().expect("UTF8 fixture path"))
                .expect("constant query engine");
        let session = cassie.create_session("tester", None);
        let mut controls = crate::runtime::QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            std::time::Instant::now(),
        );
        controls.deadline = Some(
            std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(1))
                .expect("expired test deadline"),
        );
        // Act
        let result = cassie.execute_sql_with_controls(
            &session,
            "SELECT NULL IS NOT DISTINCT FROM NULL AS same",
            vec![],
            crate::runtime::ExecutionMode::SimpleQuery,
            &controls,
        );
        // Assert
        assert!(
            matches!(result, Err(crate::app::CassieError::DeadlineExceeded)),
            "{result:?}"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
        cassie.shutdown();
    }
    drop(directory);
}

struct QueryDirectory(std::path::PathBuf);
impl Drop for QueryDirectory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            if std::thread::panicking() {
                eprintln!("null-safe fixture cleanup during unwind failed: {error}");
            } else {
                panic!("null-safe fixture cleanup failed: {error}");
            }
        }
    }
}
