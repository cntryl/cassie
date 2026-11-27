//! A snapshot-visible scalar index containing exactly the invalid integer weights.

use cassie_storage::row_blob::RowSchema;

use super::store::Reads;
use super::{BatchRow, QueryError, SourceExecutionEnv};
use crate::catalog::{IndexKind, IndexMeta};
use crate::executor::{retained_memory, scan};
use crate::midge::adapter::ScalarIndexScanRequest;
use crate::sql::ast::{BinaryOp, Expr};
use crate::sql::ColumnIdentifierPath;
use crate::types::DataType;

pub(super) struct InvalidWeights {
    index: IndexMeta,
    schema: RowSchema,
}

impl InvalidWeights {
    pub(super) fn for_mapping(
        env: &SourceExecutionEnv<'_>,
        table: &str,
        column: &str,
        fields: &[String],
    ) -> Result<Option<Self>, QueryError> {
        env.controls.check_interruption()?;
        let midge = &env.cassie.midge;
        if env.session.is_some_and(|session| {
            session.read_transaction_is_explicit() || session.has_collection_changes(table)
        }) || env
            .controls
            .statement_read()
            .is_none_or(|owner| !owner.matches(midge, &midge.relation_database(table)))
        {
            return Ok(None);
        }

        // The stored predicate evaluates a plain row column. An identifier that
        // parses as a qualified name could fall through to another field when absent.
        let Ok(weight) = ColumnIdentifierPath::parse(column) else {
            return Ok(None);
        };
        // Point-read projection uses its field labels as row keys. Leave mappings
        // whose storage representation needs quoted labels to the existing scan.
        if weight.is_qualified()
            || weight.final_name() != column
            || fields
                .iter()
                .any(|name| ColumnIdentifierPath::stored_field_key(name) != *name)
        {
            return Ok(None);
        }

        let Some(candidate) = env
            .cassie
            .catalog
            .list_indexes(table)
            .into_iter()
            .find(|index| covers_invalid_weights(index, column))
        else {
            return Ok(None);
        };

        let Some(schema) = midge.statement_row_store_schema(table)? else {
            return Ok(None);
        };
        if !schema.fields.iter().any(|field| {
            !field.retired
                && field.name == column
                && matches!(
                    field.data_type,
                    DataType::SmallInt | DataType::Int | DataType::BigInt
                )
        }) {
            return Ok(None);
        }
        let index = midge
            .published_scalar_index(table, &candidate)?
            .filter(|stored| stored.relation_id() == Some(schema.relation_id));
        Ok(index.map(|index| Self { index, schema }))
    }

    pub(super) fn visit(
        &self,
        env: &SourceExecutionEnv<'_>,
        table: &str,
        fields: &[String],
        mut visit: impl FnMut(&BatchRow) -> Result<(), QueryError>,
    ) -> Result<Reads, QueryError> {
        let (schema, _schema_memory) = env
            .cassie
            .catalog
            .clone_schema_with_controls(table, env.controls)?;
        let projection = fields.iter().cloned().collect();
        let controls = env.controls.storage_request();
        let mut reads = Reads::default();
        env.cassie.midge.visit_scalar_index_documents(
            &self.index,
            &ScalarIndexScanRequest::default(),
            &controls,
            |hit| {
                reads.index_entries += 1;
                reads.row_fetches += 1;
                if let Some(source) = env.cassie.midge.get_statement_projected_row(
                    table,
                    &self.schema,
                    &projection,
                    &hit.id,
                    &controls,
                )? {
                    let document = source.document();
                    let _memory = env.controls.reserve_query_memory(retained_memory::add(
                        64,
                        scan::projected_row_bytes(document, fields, schema.as_ref())?,
                    )?)?;
                    let row = scan::projected_document_to_row(document, fields, schema.as_ref());
                    drop(source);
                    env.controls.check_interruption()?;
                    visit(&row)?;
                }
                Ok::<_, QueryError>(true)
            },
        )?;
        Ok(reads)
    }
}

fn covers_invalid_weights(index: &IndexMeta, weight: &str) -> bool {
    if index.kind != IndexKind::Scalar
        || index.access_method() != "btree"
        || index.unique
        || !index.fields.is_empty()
        || !index.field.is_empty()
        || !index.include_fields.is_empty()
    {
        return false;
    }
    let [key] = index.expressions.as_slice() else {
        return false;
    };
    if !matches!(
        serde_json::from_str::<Expr>(key),
        Ok(Expr::IntegerLiteral(1))
    ) {
        return false;
    }
    let Some(Expr::Binary {
        left,
        op: BinaryOp::Or,
        right,
    }) = index
        .predicate
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Expr>(raw).ok())
    else {
        return false;
    };
    let column = |expr: &Expr| matches!(expr, Expr::Column(name) if name == weight);
    matches!(left.as_ref(), Expr::IsNull { expr, negated: false } if column(expr))
        && matches!(
            right.as_ref(),
            Expr::Binary { left, op: BinaryOp::Lt, right }
                if column(left) && matches!(right.as_ref(), Expr::IntegerLiteral(0))
        )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Instant;

    use super::{InvalidWeights, QueryError, SourceExecutionEnv};
    use crate::executor::execution::graph_memory_test_observer as observer;
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

    #[test]
    fn should_reject_interruption_before_choosing_weight_validation() {
        // Arrange
        let (cassie, session) = observer::fixture();
        let cancellation = QueryCancellationHandle::new();
        cancellation.cancel();
        let controls = QueryExecutionControls::with_cancellation(
            &cassie.runtime.limits(),
            Instant::now(),
            cancellation,
        );
        let functions = HashMap::new();
        let env = SourceExecutionEnv {
            cassie: &cassie,
            session: Some(&session),
            binding_context: None,
            user_functions: &functions,
            params: &[],
            parameter_type_oids: &[],
            controls: &controls,
        };

        // Act
        let result = InvalidWeights::for_mapping(&env, "e", "id", &[]);

        // Assert
        assert!(matches!(result, Err(QueryError::Cassie(error)) if error.sql_state() == "57014"));
    }
}
