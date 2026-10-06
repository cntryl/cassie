use crate::types::Value;

pub(super) fn scalar_bag(
    columns: &[Vec<Value>],
    predicate: &crate::sql::ast::Expr,
) -> Vec<Vec<Value>> {
    let functions = std::collections::HashMap::new();
    (0..columns[0].len())
        .filter_map(|index| {
            let row = crate::executor::batch::BatchRow::from_projected_values(vec![
                ("n".to_owned(), columns[0][index].clone()),
                ("flag".to_owned(), columns[1][index].clone()),
            ]);
            let truth = crate::executor::filter::evaluate_expr_value(
                &row,
                predicate,
                &[],
                None,
                &functions,
                None,
                None,
            )
            .expect("scalar oracle");
            (truth == Value::Bool(true))
                .then(|| vec![columns[0][index].clone(), columns[0][index].clone()])
        })
        .collect::<Vec<_>>()
}
