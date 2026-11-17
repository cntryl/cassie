//! Resolve statistics and whole-ON dependencies once, before searching orders.

use std::collections::HashMap;
use std::hash::BuildHasher;

use crate::catalog::{split_identifier_path, CollectionCardinalityStats};
use crate::sql::ast::{BinaryOp, Expr, IdentifierPath, QuerySource};
use crate::sql::ColumnIdentifierPath;

pub(super) struct JoinInputs {
    pub(super) rows: Vec<u64>,
    pub(super) labels: Vec<String>,
    pub(super) predicates: Vec<PredicateInput>,
}

pub(super) struct PredicateInput {
    dependencies: Option<Vec<usize>>,
    pub(super) equalities: Vec<EqualityInput>,
}

impl PredicateInput {
    pub(super) fn ready(&self, prefix: &[usize], next: usize, last: bool) -> Option<bool> {
        self.dependencies.as_ref().map(|dependencies| {
            dependencies
                .iter()
                .all(|index| *index == next || prefix.contains(index))
                && (last
                    || (dependencies.contains(&next)
                        && dependencies.iter().any(|index| prefix.contains(index))))
        })
    }
}

pub(super) struct EqualityInput {
    left: usize,
    right: usize,
    left_ndv: u64,
    right_ndv: u64,
}

impl EqualityInput {
    pub(super) fn divisor(&self, prefix: &[usize], next: usize, rows: u128) -> Option<u128> {
        let (prior, incoming) = if self.left == next && prefix.contains(&self.right) {
            (self.right_ndv, self.left_ndv)
        } else if self.right == next && prefix.contains(&self.left) {
            (self.left_ndv, self.right_ndv)
        } else {
            return None;
        };
        Some(u128::from(prior).min(rows).max(u128::from(incoming)))
    }
}

pub(super) fn resolve<S: BuildHasher>(
    relations: &[QuerySource],
    predicates: &[Expr],
    statistics: &HashMap<String, CollectionCardinalityStats, S>,
) -> JoinInputs {
    let sources = relations
        .iter()
        .map(|source| match source {
            QuerySource::Collection(name) => name,
            _ => unreachable!("join eligibility admits only collections"),
        })
        .collect::<Vec<_>>();
    let stats = sources
        .iter()
        .map(|source| relation_statistics(source, statistics))
        .collect::<Vec<_>>();
    let rows = stats
        .iter()
        .map(|entry| entry.map_or(1, |stats| stats.row_count.max(1)))
        .collect();
    let labels = sources.iter().map(|source| source.to_string()).collect();
    let mut resolver = Resolver {
        sources,
        stats,
        columns: HashMap::new(),
    };
    let predicates = predicates
        .iter()
        .map(|predicate| resolver.predicate(predicate))
        .collect();
    JoinInputs {
        rows,
        labels,
        predicates,
    }
}

fn same_identity(left: &IdentifierPath, right: &[String]) -> bool {
    left.values().eq(right.iter().map(String::as_str))
}

fn compatible_suffix(left: &IdentifierPath, right: &[String]) -> bool {
    left.components()
        .iter()
        .rev()
        .zip(right.iter().rev())
        .all(|(left, right)| left.value() == right.as_str())
}

fn relation_statistics<'a, S: BuildHasher>(
    source: &IdentifierPath,
    statistics: &'a HashMap<String, CollectionCardinalityStats, S>,
) -> Option<&'a CollectionCardinalityStats> {
    if let Some(exact) = statistics.get(source.as_str()) {
        return Some(exact);
    }
    let mut exact = None;
    let mut suffix = None;
    let mut exact_count = 0usize;
    let mut suffix_count = 0usize;
    for (key, stats) in statistics {
        let Ok(name) = split_identifier_path(key) else {
            continue;
        };
        if same_identity(source, &name) {
            exact = Some(stats);
            exact_count += 1;
        } else if compatible_suffix(source, &name) {
            suffix = Some(stats);
            suffix_count += 1;
        }
    }
    if exact_count == 1 {
        exact
    } else if exact_count == 0 && suffix_count == 1 {
        suffix
    } else {
        None
    }
}

#[derive(Clone, Copy)]
struct ColumnInput {
    relation: usize,
    ndv: Option<u64>,
}

struct Resolver<'a> {
    sources: Vec<&'a IdentifierPath>,
    stats: Vec<Option<&'a CollectionCardinalityStats>>,
    columns: HashMap<String, Option<ColumnInput>>,
}

impl Resolver<'_> {
    fn column(&mut self, raw: &str) -> Option<ColumnInput> {
        if let Some(resolved) = self.columns.get(raw) {
            return *resolved;
        }
        let result = self.resolve_column(raw);
        self.columns.insert(raw.to_string(), result);
        result
    }

    fn resolve_column(&self, raw: &str) -> Option<ColumnInput> {
        let column = ColumnIdentifierPath::parse(raw).ok()?;
        if !column.is_qualified() {
            return None;
        }
        // Remove the component produced by the existing serializer, rather
        // than splitting raw SQL at a dot inside a delimited field name.
        let namespace = column.namespace_key();
        let field_key = column.field_lookup_key();
        let qualifier = namespace.strip_suffix(&field_key)?.strip_suffix('.')?;
        let qualifier = split_identifier_path(qualifier).ok()?;
        let relation = self.relation(&qualifier)?;
        let ndv = self.stats[relation].and_then(|stats| field_ndv(stats, &column.declared_name()));
        Some(ColumnInput { relation, ndv })
    }

    fn relation(&self, qualifier: &[String]) -> Option<usize> {
        let mut exact = None;
        let mut suffix = None;
        let mut exact_count = 0usize;
        let mut suffix_count = 0usize;
        for (index, source) in self.sources.iter().enumerate() {
            if same_identity(source, qualifier) {
                exact = Some(index);
                exact_count += 1;
            } else if compatible_suffix(source, qualifier) {
                suffix = Some(index);
                suffix_count += 1;
            }
        }
        if exact_count == 1 {
            exact
        } else if exact_count == 0 && suffix_count == 1 {
            suffix
        } else {
            None
        }
    }

    fn predicate(&mut self, expr: &Expr) -> PredicateInput {
        let mut dependencies = Vec::new();
        let mut measurable = true;
        let known = self.dependencies(expr, &mut dependencies, &mut measurable);
        dependencies.sort_unstable();
        dependencies.dedup();
        let mut equalities = Vec::new();
        if known && measurable {
            self.equalities(expr, &mut equalities);
        }
        PredicateInput {
            dependencies: known.then_some(dependencies),
            equalities,
        }
    }

    fn dependencies(
        &mut self,
        expr: &Expr,
        dependencies: &mut Vec<usize>,
        measurable: &mut bool,
    ) -> bool {
        match expr {
            Expr::Column(raw) => {
                let Some(column) = self.column(raw) else {
                    return false;
                };
                dependencies.push(column.relation);
                return true;
            }
            Expr::Exists(_) | Expr::Subquery(_) => return false,
            Expr::Function(_) => *measurable = false,
            _ => {}
        }
        let mut known = true;
        expr.for_each_child(|child| {
            known &= self.dependencies(child, dependencies, measurable);
        });
        known
    }

    fn equalities(&mut self, expr: &Expr, equalities: &mut Vec<EqualityInput>) {
        match expr {
            Expr::Binary {
                left,
                op: BinaryOp::And,
                right,
            } => {
                self.equalities(left, equalities);
                self.equalities(right, equalities);
            }
            Expr::Binary {
                left,
                op: BinaryOp::Eq,
                right,
            } => {
                let (Expr::Column(left), Expr::Column(right)) = (left.as_ref(), right.as_ref())
                else {
                    return;
                };
                let (Some(left), Some(right)) = (self.column(left), self.column(right)) else {
                    return;
                };
                let (Some(left_ndv), Some(right_ndv)) = (left.ndv, right.ndv) else {
                    return;
                };
                if left.relation != right.relation {
                    equalities.push(EqualityInput {
                        left: left.relation,
                        right: right.relation,
                        left_ndv,
                        right_ndv,
                    });
                }
            }
            _ => {}
        }
    }
}

fn field_ndv(stats: &CollectionCardinalityStats, name: &str) -> Option<u64> {
    let field = stats.fields.get(name)?;
    (stats.hydrated
        && field.stale_reason.is_none()
        && (field.sample_count > 0 || field.confidence > 0)
        && field.distinct_count > 0
        && field.distinct_count <= field.non_null_count
        && field.non_null_count == stats.row_count
        && field.null_count == 0
        && field.missing_count == 0)
        .then_some(field.distinct_count)
}
