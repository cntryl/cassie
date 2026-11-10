//! Narrow same-table IF NOT EXISTS column checks.
use super::{CassieError, Catalog, CollectionSchema, Expr};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::CreateIndexStatement;

fn check(controls: Option<&QueryExecutionControls>) -> Result<(), CassieError> {
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    Ok(())
}

enum PredicateAtom<'a> {
    Column(&'a str),
    Literal,
}

fn atom(expression: &Expr) -> Option<PredicateAtom<'_>> {
    match expression {
        Expr::Column(name) => Some(PredicateAtom::Column(name)),
        Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::IntegerLiteral(_)
        | Expr::NumericLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null => Some(PredicateAtom::Literal),
        _ => None,
    }
}

fn column(atom: &PredicateAtom<'_>) -> Option<&str> {
    match atom {
        PredicateAtom::Column(name) => Some(name),
        PredicateAtom::Literal => None,
    }
}

fn predicate_columns(expression: &Expr) -> Option<[Option<&str>; 2]> {
    match expression {
        Expr::Column(name) => Some([Some(name), None]),
        Expr::Binary { left, right, .. } => Some([column(&atom(left)?), column(&atom(right)?)])
        _ => None,
    }
}

fn unqualified(name: &str) -> bool {
    crate::sql::ColumnIdentifierPath::parse(name).is_ok_and(|path| !path.is_qualified())
}

fn validate_field(
    schema: &CollectionSchema,
    name: &str,
    controls: Option<&QueryExecutionControls>,
) -> Result<(), CassieError> {
    let reference = crate::sql::ColumnIdentifierPath::reference_field_key(name);
    for field in &schema.fields {
        check(controls)?;
        if crate::sql::ColumnIdentifierPath::stored_field_key(&field.name) == reference {
            return Ok(());
        }
    }
    Err(CassieError::UndefinedColumn(name.to_owned()))
}

pub(super) fn validate(
    statement: &CreateIndexStatement,
    catalog: &Catalog,
    table: &str,
    controls: Option<&QueryExecutionControls>,
) -> Result<(), CassieError> {
    check(controls)?;
    if statement.kind != crate::catalog::IndexKind::Scalar
        || !statement.options.is_empty()
        || !statement.expressions.is_empty()
    {
        return Ok(());
    }
    let predicate = if let Some(predicate) = &statement.predicate {
        let Some(columns) = predicate_columns(predicate) else {
            return Ok(());
        };
        if columns.iter().all(Option::is_none) {
            return Ok(());
        }
        for name in columns.iter().flatten() {
            check(controls)?;
            if !unqualified(name) {
                return Ok(());
            }
        }
        Some(columns)
    } else {
        for name in statement.fields.iter().chain(&statement.include_fields) {
            check(controls)?;
            if !unqualified(name) {
                return Ok(());
            }
        }
        None
    };
    let memory;
    let schema;
    if let Some(controls) = controls {
        let (candidate, lease) = catalog.clone_schema_with_controls(table, controls)?;
        memory = lease;
        schema = candidate.ok_or_else(|| CassieError::CollectionNotFound(table.to_owned()))?;
    } else {
        memory = None;
        schema = catalog
            .get_schema(table)
            .ok_or_else(|| CassieError::CollectionNotFound(table.to_owned()))?;
    }
    check(controls)?;
    if let Some(columns) = predicate {
        for name in columns.into_iter().flatten() {
            validate_field(&schema, name, controls)?;
        }
        return Ok(());
    }
    for field in &statement.fields {
        validate_field(&schema, field, controls)?;
    }
    for field in &statement.include_fields {
        validate_field(&schema, field, controls)?;
    }
    drop(schema);
    drop(memory);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::app::CassieError;
    use crate::catalog::Catalog;
    use crate::config::CassieRuntimeLimits;
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
    use crate::sql::ast::{CreateIndexStatement, QueryStatement};
    use std::time::Instant;

    fn statement() -> CreateIndexStatement {
        let parsed =
            crate::sql::parse_statement("CREATE INDEX IF NOT EXISTS stable_idx ON rows (missing)")
                .expect("parse");
        let QueryStatement::CreateIndex(statement) = parsed.statement else {
            panic!("index statement")
        };
        statement
    }

    #[test]
    fn should_stop_duplicate_column_validation_when_cancelled() {
        // Arrange
        let catalog = Catalog::new();
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        cancellation.cancel();

        // Act
        let result = validate(&statement(), &catalog, "rows", Some(&controls));

        // Assert
        assert!(matches!(result, Err(CassieError::QueryCancelled)));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_release_duplicate_schema_admission_after_budget_failure() {
        // Arrange
        let catalog = Catalog::new();
        catalog.register_collection("rows", vec![("v".to_owned(), crate::types::DataType::Int)]);
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: 1,
            ..CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());

        // Act
        let result = validate(&statement(), &catalog, "rows", Some(&controls));

        // Assert
        assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_report_cancellation_before_skipping_an_excluded_duplicate_shape() {
        // Arrange
        let parsed = crate::sql::parse_statement(
            "CREATE INDEX IF NOT EXISTS stable_idx ON rows (v) WITH (unknown_option = 1)",
        )
        .expect("parse");
        let QueryStatement::CreateIndex(statement) = parsed.statement else {
            panic!("index statement")
        };
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        cancellation.cancel();

        // Act
        let result = validate(&statement, &Catalog::new(), "rows", Some(&controls));

        // Assert
        assert!(matches!(result, Err(CassieError::QueryCancelled)));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
