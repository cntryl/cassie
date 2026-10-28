//! Rows of a mutation target joined to its source (`UPDATE ... FROM`,
//! `DELETE ... USING`, `MERGE`).
//!
//! The target is read once from the statement snapshot and the bound source
//! query is run once; matching then pairs them, using a hash of equality
//! conjuncts when every key is a plain scalar and evaluating the full condition
//! on each candidate pair.

use std::collections::HashMap as StdHashMap;

use super::{
    batch, check_timeout, ensure_query_memory_budget, execute_plan, filter, scan, BatchRow, Cassie,
    CassieSession, CollectionSchema, CteContext, Expr, FieldMeta, FunctionMeta, HashMap,
    LogicalPlan, QueryError, QueryExecutionControls, QuerySource, Value,
};
use crate::sql::ast::{MutationSource, MutationSourceField, ReturningAliases, SelectStatement};
use crate::types::row_identity::ROW_IDENTITY_COLUMN;

/// How often matching loops observe cancellation and deadlines.
const CHECK_INTERVAL: usize = 1024;

/// The target side of a joined mutation.
pub(super) struct JoinedTarget<'a> {
    pub(super) table: &'a str,
    pub(super) qualifier: &'a str,
    pub(super) schema: &'a CollectionSchema,
}

pub(super) struct SourceRows {
    pub(super) fields: Vec<MutationSourceField>,
    pub(super) rows: Vec<Vec<Value>>,
}

impl SourceRows {
    /// A source of one empty row, for single-table statements that still bind
    /// through the joined path.
    pub(super) fn single_empty_row() -> Self {
        Self {
            fields: Vec::new(),
            rows: vec![Vec::new()],
        }
    }
}

pub(super) struct ExecutionScope<'a> {
    pub(super) cassie: &'a Cassie,
    pub(super) session: Option<&'a CassieSession>,
    pub(super) params: &'a [Value],
    pub(super) user_functions: &'a HashMap<String, FunctionMeta>,
    pub(super) controls: &'a QueryExecutionControls,
}

/// Reads every target row visible to the statement.
pub(super) fn target_rows(
    scope: &ExecutionScope<'_>,
    table: &str,
) -> Result<Vec<BatchRow>, QueryError> {
    let controls = crate::executor::execution::entrypoints::statement_read_controls(
        scope.cassie,
        scope.session,
        scope.controls,
    )?;
    let (_read_scope, _overlay_scope) =
        crate::executor::execution::entrypoints::enter_statement_read(&controls);
    let batches = scan::scan(scope.cassie, scope.session, table, &controls)?;
    ensure_query_memory_budget(&controls, &batches)?;
    Ok(batch::flatten_batches(batches))
}

/// Runs the bound source query once.
pub(super) fn source_rows(
    scope: &ExecutionScope<'_>,
    source: &MutationSource,
) -> Result<SourceRows, QueryError> {
    let select = source.query.as_deref().ok_or_else(|| {
        QueryError::General("mutation source was not bound before execution".to_string())
    })?;
    let controls = crate::executor::execution::entrypoints::statement_read_controls(
        scope.cassie,
        scope.session,
        scope.controls,
    )?;
    let (_read_scope, _overlay_scope) =
        crate::executor::execution::entrypoints::enter_statement_read(&controls);
    let mut logical = select_plan(select);
    crate::planner::logical::rewrite_reserved_id_references(&mut logical, &scope.cassie.catalog);
    let mut cte_context = CteContext::new();
    let rows = execute_plan(
        scope.cassie,
        scope.session,
        &logical,
        &mut cte_context,
        scope.user_functions,
        scope.params,
        &controls,
    )?;
    let rows = rows
        .into_iter()
        .map(|row| {
            let _operator_parent = row.operator_memory();
            row.into_entries()
                .into_iter()
                .map(|(_, value)| value)
                .collect::<Vec<_>>()
        })
        .collect();
    Ok(SourceRows {
        fields: source.fields.clone(),
        rows,
    })
}

pub(super) fn select_plan(select: &SelectStatement) -> LogicalPlan {
    LogicalPlan {
        command: None,
        source: select.source.clone(),
        collection: match &select.source {
            QuerySource::Values { .. } => "values".into(),
            QuerySource::Aliased { source, .. } | QuerySource::TableSample { source, .. } => {
                crate::planner::logical::source_name(source)
            }
            QuerySource::Collection(name) => name.to_string(),
            QuerySource::Cte(name) | QuerySource::TableFunction { name, .. } => name.clone(),
            QuerySource::Subquery { alias, .. } => alias.clone(),
            QuerySource::SingleRow => "single_row".to_string(),
            QuerySource::Join { .. } => "join".to_string(),
        },
        ctes: select.ctes.clone(),
        distinct: select.distinct,
        distinct_on: select.distinct_on.clone(),
        projection: select.projection.clone(),
        filter: select.filter.clone(),
        group_by: select.group_by.clone(),
        having: select.having.clone(),
        order: select.order.clone(),
        limit: select.limit.clone(),
        offset: select.offset.clone(),
        set: select.set.clone(),
    }
}

fn field_key(field: &FieldMeta) -> String {
    crate::sql::ColumnIdentifierPath::from_field_name(&field.name).lookup_key()
}

/// Builds one joined row: the target's columns under `qualifier.column` (and
/// bare names), the source's qualified columns, then `extras`.
pub(super) fn combined_row(
    target: &JoinedTarget<'_>,
    target_row: Option<&BatchRow>,
    source: &SourceRows,
    source_row: Option<&[Value]>,
    extras: Vec<(String, Value)>,
) -> BatchRow {
    let mut entries =
        Vec::with_capacity(target.schema.fields.len() + source.fields.len() + extras.len() + 1);
    let mut aliases = Vec::new();
    if let Some(row_id) = target_row.and_then(|row| row.get(ROW_IDENTITY_COLUMN)) {
        entries.push((ROW_IDENTITY_COLUMN.to_string(), row_id.clone()));
    }
    for field in &target.schema.fields {
        let key = field_key(field);
        let value = target_row
            .and_then(|row| row.get(&field.name))
            .cloned()
            .unwrap_or(Value::Null);
        aliases.push((key.clone(), entries.len()));
        aliases.push((field.name.clone(), entries.len()));
        entries.push((format!("{}.{key}", target.qualifier), value));
    }
    for (index, field) in source.fields.iter().enumerate() {
        let value = source_row
            .and_then(|row| row.get(index))
            .cloned()
            .unwrap_or(Value::Null);
        aliases.push((field.column.clone(), entries.len()));
        entries.push((field.qualified_name(), value));
    }
    entries.extend(extras);
    BatchRow::with_aliases(entries, aliases)
}

/// The `OLD`/`NEW` row-image entries for `RETURNING`.
pub(super) fn row_image_entries(
    schema: &CollectionSchema,
    aliases: Option<&ReturningAliases>,
    old: Option<&serde_json::Value>,
    new: Option<&serde_json::Value>,
) -> Vec<(String, Value)> {
    let Some(aliases) = aliases else {
        return Vec::new();
    };
    let mut entries = Vec::with_capacity(schema.fields.len() * 2);
    for (qualifier, payload) in [(&aliases.old, old), (&aliases.new, new)] {
        for field in &schema.fields {
            let value = payload
                .and_then(|payload| payload.get(&field.name))
                .map_or(Value::Null, |value| stored_value(field, value));
            entries.push((format!("{qualifier}.{}", field_key(field)), value));
        }
    }
    entries
}

fn stored_value(field: &FieldMeta, value: &serde_json::Value) -> Value {
    if matches!(
        field.data_type,
        crate::types::DataType::Json | crate::types::DataType::Jsonb
    ) && crate::types::json::requires_document_carrier(value)
    {
        Value::Json(value.clone())
    } else {
        super::json_to_value(value)
    }
}

/// A target row rebuilt from a stored payload, as `target_rows` reads it.
pub(super) fn payload_row(
    row_id: &str,
    schema: &CollectionSchema,
    payload: &serde_json::Value,
) -> BatchRow {
    super::inserted_row_to_batch_row(row_id, schema, payload)
}

/// For each source row, the indexes of the target rows `condition` matches.
pub(super) fn match_rows(
    scope: &ExecutionScope<'_>,
    target: &JoinedTarget<'_>,
    targets: &[BatchRow],
    source: &SourceRows,
    condition: Option<&Expr>,
) -> Result<Vec<Vec<usize>>, QueryError> {
    let condition = condition
        .map(|condition| {
            crate::executor::execution::resolve_statement_exists(
                scope.cassie,
                scope.session,
                condition,
                scope.user_functions,
                scope.params,
                scope.controls,
            )
        })
        .transpose()?;
    let candidates = condition
        .as_ref()
        .and_then(|condition| hashed_candidates(target, targets, source, condition));
    let mut matches = Vec::with_capacity(source.rows.len());
    let mut evaluated = 0usize;
    for (source_index, source_row) in source.rows.iter().enumerate() {
        let mut hits = Vec::new();
        let indexes: Box<dyn Iterator<Item = usize>> = match &candidates {
            Some(candidates) => Box::new(candidates[source_index].iter().copied()),
            None => Box::new(0..targets.len()),
        };
        for target_index in indexes {
            evaluated += 1;
            if evaluated.is_multiple_of(CHECK_INTERVAL) {
                check_timeout(scope.controls)?;
            }
            let Some(condition) = condition.as_ref() else {
                hits.push(target_index);
                continue;
            };
            let row = combined_row(
                target,
                Some(&targets[target_index]),
                source,
                Some(source_row),
                Vec::new(),
            );
            if filter::eval_scalar(
                &row,
                condition,
                scope.params,
                None,
                scope.user_functions,
                None,
                scope.session,
            )?
            .is_true()?
            {
                hits.push(target_index);
            }
        }
        matches.push(hits);
    }
    check_timeout(scope.controls)?;
    Ok(matches)
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum HashKey {
    /// SQL NULL, which equals nothing: a key holding it never matches.
    Null,
    Integer(i64),
    Text(String),
    Boolean(bool),
}

impl HashKey {
    /// The key for `value`, or `None` when its type is not hashed.
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Null => Some(Self::Null),
            Value::Int64(value) => Some(Self::Integer(*value)),
            Value::String(value) => Some(Self::Text(value.clone())),
            Value::Bool(value) => Some(Self::Boolean(*value)),
            _ => None,
        }
    }

    const fn kind(&self) -> Option<u8> {
        match self {
            Self::Null => None,
            Self::Integer(_) => Some(0),
            Self::Text(_) => Some(1),
            Self::Boolean(_) => Some(2),
        }
    }
}

fn has_null(key: &[HashKey]) -> bool {
    key.contains(&HashKey::Null)
}

/// Candidate target indexes per source row from the equality conjuncts of
/// `condition`, or `None` when the condition offers no plain scalar key.
fn hashed_candidates(
    target: &JoinedTarget<'_>,
    targets: &[BatchRow],
    source: &SourceRows,
    condition: &Expr,
) -> Option<Vec<Vec<usize>>> {
    let mut pairs = Vec::new();
    collect_key_pairs(target, source, condition, &mut pairs);
    if pairs.is_empty() {
        return None;
    }
    let target_keys = targets
        .iter()
        .map(|row| {
            pairs
                .iter()
                .map(|(field, _)| HashKey::from_value(row.get(field).unwrap_or(&Value::Null)))
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>()?;
    let source_keys = source
        .rows
        .iter()
        .map(|row| {
            pairs
                .iter()
                .map(|(_, index)| HashKey::from_value(row.get(*index).unwrap_or(&Value::Null)))
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>()?;
    if !consistent_kinds(&target_keys, &source_keys, pairs.len()) {
        return None;
    }
    let mut table = StdHashMap::<Vec<HashKey>, Vec<usize>>::new();
    for (index, key) in target_keys.into_iter().enumerate() {
        if !has_null(&key) {
            table.entry(key).or_default().push(index);
        }
    }
    Some(
        source_keys
            .into_iter()
            .map(|key| {
                if has_null(&key) {
                    Vec::new()
                } else {
                    table.get(&key).cloned().unwrap_or_default()
                }
            })
            .collect(),
    )
}

/// Each key position must carry one scalar kind on both sides, so hashing
/// agrees with the evaluator's equality.
fn consistent_kinds(
    target_keys: &[Vec<HashKey>],
    source_keys: &[Vec<HashKey>],
    width: usize,
) -> bool {
    (0..width).all(|position| {
        let mut seen = None;
        target_keys
            .iter()
            .chain(source_keys)
            .filter_map(|key| key[position].kind())
            .all(|kind| *seen.get_or_insert(kind) == kind)
    })
}

/// Collects `(target field, source index)` pairs from top-level `AND`ed
/// `target.column = source.column` conjuncts.
fn collect_key_pairs(
    target: &JoinedTarget<'_>,
    source: &SourceRows,
    condition: &Expr,
    pairs: &mut Vec<(String, usize)>,
) {
    match condition {
        Expr::Binary {
            left,
            op: crate::sql::ast::BinaryOp::And,
            right,
        } => {
            collect_key_pairs(target, source, left, pairs);
            collect_key_pairs(target, source, right, pairs);
        }
        Expr::Binary {
            left,
            op: crate::sql::ast::BinaryOp::Eq,
            right,
        } => {
            let (Expr::Column(left), Expr::Column(right)) = (left.as_ref(), right.as_ref()) else {
                return;
            };
            let sides = (
                target_field(target, source, left),
                source_index(target, source, right),
            );
            let swapped = (
                target_field(target, source, right),
                source_index(target, source, left),
            );
            if let (Some(field), Some(index)) = sides {
                pairs.push((field, index));
            } else if let (Some(field), Some(index)) = swapped {
                pairs.push((field, index));
            }
        }
        _ => {}
    }
}

fn reference(name: &str) -> Option<(Option<String>, String)> {
    let column = crate::sql::ColumnIdentifierPath::parse(name).ok()?;
    let field = column.field_lookup_key();
    let lookup = column.lookup_key();
    let qualifier = lookup
        .strip_suffix(&format!(".{field}"))
        .map(str::to_string);
    Some((qualifier, field))
}

fn target_field(target: &JoinedTarget<'_>, source: &SourceRows, name: &str) -> Option<String> {
    let (qualifier, column) = reference(name)?;
    match qualifier {
        Some(qualifier) if !qualifier.eq_ignore_ascii_case(target.qualifier) => return None,
        None if source.fields.iter().any(|field| field.column == column) => return None,
        _ => {}
    }
    target
        .schema
        .fields
        .iter()
        .find(|field| field_key(field) == column)
        .map(|field| field.name.clone())
}

fn source_index(target: &JoinedTarget<'_>, source: &SourceRows, name: &str) -> Option<usize> {
    let (qualifier, column) = reference(name)?;
    if qualifier.is_none()
        && target
            .schema
            .fields
            .iter()
            .any(|field| field_key(field) == column)
    {
        return None;
    }
    let mut candidates = source.fields.iter().enumerate().filter(|(_, field)| {
        field.column == column
            && qualifier
                .as_deref()
                .is_none_or(|qualifier| field.qualifier.eq_ignore_ascii_case(qualifier))
    });
    let (index, _) = candidates.next()?;
    candidates.next().is_none().then_some(index)
}

/// The schema `RETURNING` columns of a joined mutation resolve against.
#[must_use]
pub(crate) fn joined_returning_schema(
    schema: &CollectionSchema,
    qualifier: &str,
    source_fields: &[MutationSourceField],
    aliases: Option<&ReturningAliases>,
    merge: bool,
) -> CollectionSchema {
    let mut fields = schema.fields.clone();
    for field in &schema.fields {
        fields.push(FieldMeta {
            name: format!("{qualifier}.{}", field_key(field)),
            ..field.clone()
        });
    }
    for field in source_fields {
        fields.push(FieldMeta {
            name: field.qualified_name(),
            data_type: field.data_type.clone(),
            is_indexed: false,
            boost: None,
        });
    }
    if merge {
        fields.push(FieldMeta {
            name: crate::sql::ast::MERGE_ACTION_COLUMN.to_string(),
            data_type: crate::types::DataType::Text,
            is_indexed: false,
            boost: None,
        });
    }
    if let Some(aliases) = aliases {
        for alias in [&aliases.old, &aliases.new] {
            for field in &schema.fields {
                fields.push(FieldMeta {
                    name: format!("{alias}.{}", field_key(field)),
                    ..field.clone()
                });
            }
        }
    }
    CollectionSchema {
        collection: schema.collection.clone(),
        fields,
    }
}
