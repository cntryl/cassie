//! Arguments of `graph_weighted_paths` (#1258): the NULL, type and bound rules, in the
//! contract's order, before any catalog or table is consulted.

use super::super::super::{QueryError, Value};
use crate::app::{CassieError, DiagnosticError};
use crate::types::extended::numeric::Numeric;
use crate::types::graph::{ElementKind, ElementRef, GraphTypeKind};
use crate::types::typed::TypedValue;

pub(super) fn diagnostic(state: &'static str, message: impl Into<String>) -> QueryError {
    QueryError::Cassie(CassieError::from(DiagnosticError::new(state, message)))
}

/// Which way edges may be traversed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Way {
    Out,
    In,
    Both,
}

pub(super) struct Request {
    pub(super) graph: String,
    pub(super) source: ElementRef,
    pub(super) target: ElementRef,
    pub(super) property: String,
    pub(super) max_hops: usize,
    pub(super) way: Way,
    /// `None` selects every label; an empty list selects none.
    pub(super) labels: Option<Vec<String>>,
    pub(super) max_cost: Option<Numeric>,
    pub(super) k: usize,
}

const NAMES: [&str; 9] = [
    "graph",
    "source",
    "target",
    "weight_property",
    "max_hops",
    "direction",
    "edge_labels",
    "max_cost",
    "k",
];

fn text(value: &Value, name: &str) -> Result<String, QueryError> {
    match value {
        Value::String(text) => Ok(text.clone()),
        _ => Err(diagnostic("42804", format!("{name} must be text"))),
    }
}

fn integer(value: &Value, name: &str) -> Result<i64, QueryError> {
    match value {
        Value::Int64(number) => Ok(*number),
        _ => Err(diagnostic("42804", format!("{name} must be an integer"))),
    }
}

fn element(value: &Value, name: &str) -> Result<ElementRef, QueryError> {
    let Value::Typed(TypedValue::Record(record)) = value else {
        return Err(diagnostic(
            "42804",
            format!("{name} must be of type cassie_graph.graph_element_ref"),
        ));
    };
    if GraphTypeKind::of(record.descriptor()) != Some(GraphTypeKind::ElementRef) {
        return Err(diagnostic(
            "42804",
            format!("{name} must be of type cassie_graph.graph_element_ref"),
        ));
    }
    let reference = ElementRef::from_record(record)
        .map_err(|error| diagnostic("22023", error.message().to_owned()))?;
    if reference.kind == ElementKind::Edge {
        return Err(diagnostic(
            "22023",
            format!("{name} must be a vertex reference"),
        ));
    }
    Ok(reference)
}

fn labels(value: &Value) -> Result<Option<Vec<String>>, QueryError> {
    match value {
        Value::Null => Ok(None),
        Value::Array(array) => array
            .elements()
            .iter()
            .map(|element| match element {
                Value::String(label) => Ok(label.clone()),
                Value::Null => Err(diagnostic("22004", "edge_labels must not contain NULL")),
                _ => Err(diagnostic("42804", "edge_labels must be text[]")),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        _ => Err(diagnostic("42804", "edge_labels must be text[]")),
    }
}

fn max_cost(value: &Value) -> Result<Option<Numeric>, QueryError> {
    let numeric = match value {
        Value::Null => return Ok(None),
        Value::Int64(number) => Numeric::from_i64(*number),
        Value::Float64(number) => Numeric::from_f64(*number)
            .map_err(|_| diagnostic("22023", "max_cost must be finite and non-negative"))?,
        Value::Typed(TypedValue::Real(number)) => Numeric::from_f32(*number)
            .map_err(|_| diagnostic("22023", "max_cost must be finite and non-negative"))?,
        Value::Typed(TypedValue::Numeric(numeric)) => numeric.clone(),
        _ => return Err(diagnostic("42804", "max_cost must be numeric")),
    };
    match numeric.signum() {
        Some(sign) if sign >= 0 && matches!(numeric, Numeric::Finite { .. }) => Ok(Some(numeric)),
        _ => Err(diagnostic(
            "22023",
            "max_cost must be finite and non-negative",
        )),
    }
}

fn bounded(number: i64, name: &str, least: i64) -> Result<usize, QueryError> {
    if number < least || number > i64::from(i32::MAX) {
        return Err(diagnostic(
            "22023",
            format!("{name} must be between {least} and {}", i32::MAX),
        ));
    }
    usize::try_from(number).map_err(|_| diagnostic("22023", format!("{name} is out of range")))
}

impl Request {
    /// Validates the nine positional arguments.
    ///
    /// # Errors
    ///
    /// `22004` for a required or explicitly `NULL` argument, `42804` for a wrong type and
    /// `22023` for a bound, direction or reference-kind violation.
    pub(super) fn parse(args: &[Value]) -> Result<Self, QueryError> {
        let [graph, source, target, property, hops, way, labels_value, cost, k] = args else {
            return Err(diagnostic(
                "42883",
                "function graph_weighted_paths takes nine arguments",
            ));
        };
        for (index, value) in args.iter().enumerate() {
            if value.is_null() && !matches!(index, 6 | 7) {
                return Err(diagnostic(
                    "22004",
                    format!("argument {} must not be NULL", NAMES[index]),
                ));
            }
        }
        let labels = labels(labels_value)?;
        let graph = text(graph, "graph")?;
        let property = text(property, "weight_property")?;
        let way = match text(way, "direction")?.as_str() {
            "out" => Way::Out,
            "in" => Way::In,
            "both" => Way::Both,
            _ => return Err(diagnostic("22023", "direction must be out, in or both")),
        };
        let max_hops = bounded(integer(hops, "max_hops")?, "max_hops", 0)?;
        let k = bounded(integer(k, "k")?, "k", 1)?;
        let max_cost = max_cost(cost)?;
        Ok(Self {
            graph,
            source: element(source, "source")?,
            target: element(target, "target")?,
            property,
            max_hops,
            way,
            labels,
            max_cost,
            k,
        })
    }
}
