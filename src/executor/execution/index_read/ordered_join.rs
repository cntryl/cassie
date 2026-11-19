//! Prove an ordered fact source and unique integer dimension probes from the bound SQL tree.
use super::{
    scalar_index_read_spec, Cassie, CassieSession, Expr, IndexMeta, LogicalPlan, QueryError,
    ScalarIndexNullKeys, ScalarIndexPlanPath, ScalarIndexPredicateResolution, SelectItem,
};
use crate::catalog::{CollectionSchema, IndexKind};
use crate::midge::adapter::ScalarIndexScanRequest;
use crate::sql::ast::{BinaryOp, JoinKind, QuerySource};
use crate::sql::ColumnIdentifierPath;
use crate::types::DataType;

pub(in crate::executor::execution) const STRATEGY: &str = "ordered_index_nested_loop";

pub(in crate::executor::execution) struct Relation {
    pub collection: String,
    pub fields: Vec<String>,
    pub schema: CollectionSchema,
}

impl Relation {
    fn new(cassie: &Cassie, session: Option<&CassieSession>, collection: &str) -> Option<Self> {
        if cassie.catalog.is_materialized_projection(collection)
            || cassie.catalog.get_view(collection).is_some()
            || session.is_some_and(|s| s.has_collection_changes(collection))
        {
            return None;
        }
        Some(Self {
            collection: collection.into(),
            fields: Vec::new(),
            schema: cassie.catalog.get_schema(collection)?,
        })
    }

    fn require(&mut self, field: &str) -> Option<()> {
        let stored = &self
            .schema
            .fields
            .iter()
            .find(|f| ColumnIdentifierPath::matches_stored_field(field, &f.name))?
            .name;
        if !self.fields.contains(stored) {
            self.fields.push(stored.clone());
        }
        Some(())
    }
}

pub(in crate::executor::execution) struct Probe {
    pub input: String,
    pub index: IndexMeta,
    pub relation: Relation,
}

pub(in crate::executor::execution) struct OrderedJoin {
    pub source: Relation,
    pub index: IndexMeta,
    pub request: ScalarIndexScanRequest,
    pub timestamp_fields: Vec<String>,
    pub probes: Vec<Probe>,
    pub residuals: Vec<Expr>,
    pub limit: usize,
}

impl Cassie {
    pub(crate) fn ordered_join_source_index(
        &self,
        session: Option<&CassieSession>,
        plan: &LogicalPlan,
    ) -> Result<Option<String>, QueryError> {
        Ok(prepare(self, session, plan)?.map(|join| join.index.name))
    }
}

fn split_source<'a>(
    source: &'a QuerySource,
    edges: &mut Vec<(&'a str, &'a Expr)>,
) -> Option<&'a QuerySource> {
    match source {
        QuerySource::Collection(_) => Some(source),
        QuerySource::Join {
            left,
            right,
            kind: JoinKind::Inner,
            on,
        } => {
            let first = split_source(left, edges)?;
            let QuerySource::Collection(right) = right.as_ref() else {
                return None;
            };
            edges.push((right.as_str(), on));
            Some(first)
        }
        _ => None,
    }
}

struct JoinShape {
    source: Relation,
    probes: Vec<Probe>,
    source_plan: LogicalPlan,
    residuals: Vec<Expr>,
}

pub(in crate::executor::execution) fn prepare(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
) -> Result<Option<OrderedJoin>, QueryError> {
    let Some(limit) = plan.limit_value().and_then(|n| usize::try_from(n).ok()) else {
        return Ok(None);
    };
    if !matches!(plan.source, QuerySource::Join { .. })
        || plan.order.is_empty()
        || plan.offset.is_some()
        || plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
    {
        return Ok(None);
    }
    let Some(shape) = join_shape(cassie, session, plan) else {
        return Ok(None);
    };
    let Some(spec) = scalar_index_read_spec(cassie, session, None, &shape.source_plan, &[], true)?
    else {
        return Ok(None);
    };
    if spec.path != ScalarIndexPlanPath::OrderedBoundedScan
        || spec.null_keys != ScalarIndexNullKeys::Excluded
        || !spec.covered
        || !spec.sort_applied
        || spec.request.equality_prefix.is_empty()
        || !matches!(
            spec.predicate_resolution,
            ScalarIndexPredicateResolution::Exact
        )
        || session.is_some_and(|s| !s.transaction_index_visible(&spec.index, &cassie.catalog))
    {
        return Ok(None);
    }
    // The source LIMIT proves ordered access, but only the final joined result may stop the cursor.
    let request = ScalarIndexScanRequest {
        limit: (limit == 0).then_some(0),
        ..spec.request
    };
    Ok(Some(OrderedJoin {
        source: shape.source,
        index: spec.index,
        request,
        timestamp_fields: spec.timestamp_fields,
        probes: shape.probes,
        residuals: shape.residuals,
        limit,
    }))
}

fn join_shape(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
) -> Option<JoinShape> {
    let mut edges = Vec::new();
    let source = split_source(&plan.source, &mut edges)?;
    let QuerySource::Collection(fact) = source else {
        return None;
    };
    let mut relations = vec![Relation::new(cassie, session, fact)?];
    let mut indexes = Vec::new();
    for (dimension, on) in edges {
        relations.push(Relation::new(cassie, session, dimension)?);
        // A repeated physical name requires SQL aliases; this path accepts bare collections only.
        if relations[..relations.len() - 1]
            .iter()
            .any(|r| r.collection == dimension)
        {
            return None;
        }
        let (input, key) = edge_columns(on, &relations)?;
        let (_, field) = resolve(input, &relations)?;
        relations[0].require(&field)?;
        let last = relations.last_mut()?;
        let key_type = cassie.catalog.field_type(dimension, &key)?;
        if !matches!(
            key_type,
            DataType::Int | DataType::SmallInt | DataType::BigInt
        ) || cassie.catalog.field_type(fact, &field)? != key_type
        {
            return None;
        }
        let index = cassie
            .catalog
            .list_indexes(dimension)
            .into_iter()
            .find(|i| {
                i.kind == IndexKind::Scalar
                    && i.unique
                    && i.predicate.is_none()
                    && i.expressions.is_empty()
                    && i.normalized_fields().as_slice() == [key.as_str()]
                    && session.is_none_or(|s| s.transaction_index_visible(i, &cassie.catalog))
            })?;
        last.require(&key)?;
        indexes.push((input.to_owned(), index));
    }
    for item in &plan.projection {
        let SelectItem::Column { name, .. } = item else {
            return None;
        };
        let (relation, field) = resolve(name, &relations)?;
        relations[relation].require(&field)?;
    }
    let mut source_plan = plan.clone();
    source_plan.source = source.clone();
    source_plan.collection = fact.to_string();
    for order in &mut source_plan.order {
        let Expr::Column(name) = &order.expr else {
            return None;
        };
        let (relation, field) = resolve(name, &relations)?;
        if relation != 0 {
            return None;
        }
        order.expr = Expr::Column(field);
    }
    let mut local = None;
    let mut residuals = Vec::new();
    split_filter(
        plan.filter.as_ref()?,
        &mut relations,
        &mut local,
        &mut residuals,
    )?;
    source_plan.filter = local;
    let mut relations = relations.into_iter();
    let source = relations.next()?;
    source_plan.projection = source
        .fields
        .iter()
        .map(|field| SelectItem::Column {
            name: ColumnIdentifierPath::stored_field_key(field),
            alias: None,
        })
        .collect();
    let probes = indexes
        .into_iter()
        .zip(relations)
        .map(|((input, index), relation)| Probe {
            input,
            index,
            relation,
        })
        .collect();
    Some(JoinShape {
        source,
        probes,
        source_plan,
        residuals,
    })
}

fn edge_columns<'a>(on: &'a Expr, relations: &[Relation]) -> Option<(&'a str, String)> {
    let Expr::Binary {
        left,
        op: BinaryOp::Eq,
        right,
    } = on
    else {
        return None;
    };
    let (Expr::Column(left), Expr::Column(right)) = (left.as_ref(), right.as_ref()) else {
        return None;
    };
    let last = relations.len() - 1;
    for (input, dimension) in [(left, right), (right, left)] {
        let (i, _) = resolve(input, relations)?;
        let (j, key) = resolve(dimension, relations)?;
        if i == 0 && j == last {
            return Some((input, key));
        }
    }
    None
}

fn resolve(raw: &str, relations: &[Relation]) -> Option<(usize, String)> {
    let parts = crate::catalog::split_identifier_path(raw).ok()?;
    let (field, prefix) = parts.split_last()?;
    if prefix.is_empty() {
        return None;
    }
    let mut matching = relations.iter().enumerate().filter(|(_, r)| {
        crate::catalog::split_identifier_path(&r.collection)
            .is_ok_and(|name| name.ends_with(prefix))
    });
    let (index, _) = matching.next()?;
    matching
        .next()
        .is_none()
        .then(|| (index, ColumnIdentifierPath::stored_field_key(field)))
}

fn split_filter(
    term: &Expr,
    relations: &mut [Relation],
    local: &mut Option<Expr>,
    residuals: &mut Vec<Expr>,
) -> Option<()> {
    let Expr::Binary { left, op, right } = term else {
        return None;
    };
    if matches!(op, BinaryOp::And) {
        split_filter(left, relations, local, residuals)?;
        return split_filter(right, relations, local, residuals);
    }
    let Expr::Column(column) = left.as_ref() else {
        return None;
    };
    if !matches!(
        right.as_ref(),
        Expr::StringLiteral(_) | Expr::IntegerLiteral(_) | Expr::BoolLiteral(_)
    ) || !matches!(
        op,
        BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Gt
            | BinaryOp::Gte
            | BinaryOp::Lt
            | BinaryOp::Lte
    ) {
        return None;
    }
    let (relation, field) = resolve(column, relations)?;
    if relation == 0 {
        let part = Expr::Binary {
            left: Box::new(Expr::Column(field)),
            op: op.clone(),
            right: right.clone(),
        };
        *local = Some(if let Some(previous) = local.take() {
            Expr::Binary {
                left: Box::new(previous),
                op: BinaryOp::And,
                right: Box::new(part),
            }
        } else {
            part
        });
    } else {
        relations[relation].require(&field)?;
        residuals.push(term.clone());
    }
    Some(())
}
