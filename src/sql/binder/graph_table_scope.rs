//! Pattern variables of a `GRAPH_TABLE` and the mappings and properties they select (#1256).

use super::{CassieError, DataType};
use crate::app::DiagnosticError;
use crate::catalog::{GraphLabelMeta, PropertyGraphMeta};
use crate::sql::ast::{GraphTable, GraphVariableKind};

const UNDEFINED_OBJECT: &str = "42704";
const UNDEFINED_COLUMN: &str = "42703";
const DUPLICATE_ALIAS: &str = "42712";
const DATATYPE_MISMATCH: &str = "42804";
const UNSUPPORTED: &str = "0A000";

pub(super) fn diagnostic(state: &'static str, message: impl Into<String>) -> CassieError {
    CassieError::from(DiagnosticError::new(state, message))
}

pub(super) fn unsupported(message: impl Into<String>) -> CassieError {
    diagnostic(UNSUPPORTED, message)
}

/// One mapping a pattern element can bind, with the listed labels it carries.
#[derive(Clone)]
pub(super) struct Element<'g> {
    pub(super) mapping_id: &'g str,
    pub(super) table: &'g str,
    pub(super) labels: Vec<&'g GraphLabelMeta>,
    /// Vertex mapping aliases an edge mapping connects; empty for vertices.
    pub(super) endpoints: [&'g str; 2],
}

/// A named pattern element.
pub(super) struct Variable<'g> {
    pub(super) name: String,
    pub(super) kind: GraphVariableKind,
    pub(super) group: bool,
    pub(super) elements: Vec<Element<'g>>,
}

fn label_requested(requested: &[String], label: &GraphLabelMeta) -> bool {
    requested.is_empty()
        || requested
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&label.name))
}

/// The mappings of one kind that carry any of `requested` (all mappings when empty).
/// A mapping carrying several requested labels is one candidate.
pub(super) fn select_elements<'g>(
    graph: &'g PropertyGraphMeta,
    kind: GraphVariableKind,
    requested: &[String],
) -> Result<Vec<Element<'g>>, CassieError> {
    let mut candidates = Vec::new();
    if kind == GraphVariableKind::Vertex {
        for vertex in &graph.vertices {
            candidates.push((&vertex.mapping_id, &vertex.table, &vertex.labels, ["", ""]));
        }
    } else {
        for edge in &graph.edges {
            candidates.push((
                &edge.mapping_id,
                &edge.table,
                &edge.labels,
                [
                    edge.source.vertex_alias.as_str(),
                    edge.destination.vertex_alias.as_str(),
                ],
            ));
        }
    }
    for name in requested {
        let known = candidates.iter().any(|(_, _, labels, _)| {
            labels
                .iter()
                .any(|label| label.name.eq_ignore_ascii_case(name))
        });
        if !known {
            let what = if kind == GraphVariableKind::Vertex {
                "vertex"
            } else {
                "edge"
            };
            return Err(diagnostic(
                UNDEFINED_OBJECT,
                format!("{what} label \"{name}\" does not exist in the property graph"),
            ));
        }
    }
    Ok(candidates
        .into_iter()
        .filter_map(|(mapping_id, table, labels, endpoints)| {
            let carried = labels
                .iter()
                .filter(|label| label_requested(requested, label))
                .collect::<Vec<_>>();
            (!carried.is_empty()).then_some(Element {
                mapping_id,
                table,
                labels: carried,
                endpoints,
            })
        })
        .collect())
}

/// The declared variables of `table`, merging repeated singleton occurrences, and the
/// elements of every pattern position (named or not) for table access.
pub(super) fn collect<'g>(
    graph: &'g PropertyGraphMeta,
    table: &GraphTable,
) -> Result<(Vec<Variable<'g>>, Vec<Element<'g>>), CassieError> {
    let mut variables = Vec::<Variable<'g>>::new();
    let mut everything = Vec::new();
    let positions = std::iter::once((
        GraphVariableKind::Vertex,
        &table.path.start.variable,
        &table.path.start.labels,
        false,
    ))
    .chain(table.path.steps.iter().flat_map(|step| {
        [
            (
                GraphVariableKind::Edge,
                &step.edge.variable,
                &step.edge.labels,
                step.edge.quantifier.is_some(),
            ),
            (
                GraphVariableKind::Vertex,
                &step.vertex.variable,
                &step.vertex.labels,
                false,
            ),
        ]
    }));
    for (kind, name, labels, group) in positions {
        let elements = select_elements(graph, kind, labels)?;
        everything.extend(elements.iter().cloned());
        let Some(name) = name else {
            continue;
        };
        declare(&mut variables, name, kind, group, elements)?;
    }
    if let Some(path) = &table.path_variable {
        if variables.iter().any(|variable| &variable.name == path) {
            return Err(diagnostic(
                DUPLICATE_ALIAS,
                format!("path variable \"{path}\" is also used for a pattern element"),
            ));
        }
        variables.push(Variable {
            name: path.clone(),
            kind: GraphVariableKind::Path,
            group: false,
            elements: Vec::new(),
        });
    }
    Ok((variables, everything))
}

fn declare<'g>(
    variables: &mut Vec<Variable<'g>>,
    name: &str,
    kind: GraphVariableKind,
    group: bool,
    elements: Vec<Element<'g>>,
) -> Result<(), CassieError> {
    let Some(existing) = variables.iter_mut().find(|variable| variable.name == name) else {
        variables.push(Variable {
            name: name.to_owned(),
            kind,
            group,
            elements,
        });
        return Ok(());
    };
    if existing.kind != kind {
        return Err(diagnostic(
            DUPLICATE_ALIAS,
            format!("variable \"{name}\" cannot name both a vertex and an edge"),
        ));
    }
    if existing.group || group {
        return Err(unsupported(format!(
            "repeating the quantified edge variable \"{name}\" is not supported"
        )));
    }
    // The same element at every occurrence: it must be selected by each one.
    let mut merged = Vec::new();
    for mut element in std::mem::take(&mut existing.elements) {
        if let Some(other) = elements
            .iter()
            .find(|candidate| candidate.mapping_id == element.mapping_id)
        {
            for label in &other.labels {
                if !element.labels.iter().any(|known| known.name == label.name) {
                    element.labels.push(label);
                }
            }
            merged.push(element);
        }
    }
    existing.elements = merged;
    Ok(())
}

/// The type of `property` on `variable`: exposed by every label of every selected mapping,
/// with one type.
pub(super) fn property_type(
    variable: &Variable<'_>,
    property: &str,
) -> Result<DataType, CassieError> {
    let mut found: Option<DataType> = None;
    for element in &variable.elements {
        for label in &element.labels {
            let Some(exposed) = label
                .properties
                .iter()
                .find(|candidate| candidate.name.eq_ignore_ascii_case(property))
            else {
                return Err(diagnostic(
                    UNDEFINED_COLUMN,
                    format!(
                        "property \"{property}\" of variable \"{}\" is not exposed by every \
                         selected mapping",
                        variable.name
                    ),
                ));
            };
            let data_type = exposed.data_type.flattened().clone();
            match &found {
                Some(previous) if *previous != data_type => {
                    return Err(diagnostic(
                        DATATYPE_MISMATCH,
                        format!(
                            "property \"{property}\" of variable \"{}\" has incompatible types \
                             across the selected mappings",
                            variable.name
                        ),
                    ));
                }
                _ => found = Some(data_type),
            }
        }
    }
    found.ok_or_else(|| {
        diagnostic(
            UNDEFINED_COLUMN,
            format!(
                "property \"{property}\" of variable \"{}\" is not exposed by any selected \
                 mapping",
                variable.name
            ),
        )
    })
}

/// Properties of `variable` that bind to one type, in first-seen order.
pub(super) fn consistent_properties(variable: &Variable<'_>) -> Vec<(String, DataType)> {
    let mut names = Vec::<&str>::new();
    for property in variable
        .elements
        .iter()
        .flat_map(|element| &element.labels)
        .flat_map(|label| &label.properties)
    {
        if !names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&property.name))
        {
            names.push(&property.name);
        }
    }
    names
        .into_iter()
        .filter_map(|name| {
            property_type(variable, name)
                .ok()
                .map(|data_type| (name.to_owned(), data_type))
        })
        .collect()
}
