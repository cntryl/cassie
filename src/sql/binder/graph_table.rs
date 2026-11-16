//! Binding of the `GRAPH_TABLE` table source against a mapped property graph (#1256).
//!
//! Binding resolves the graph, the mappings each label expression selects, the properties
//! each variable exposes and the output schema. It never reads graph data: execution is a
//! separate step.

use super::graph_table_expr::{self as expr, Scope};
use super::graph_table_scope::{self as scope, diagnostic, unsupported, Variable};
use super::{BindingContext, CassieError, Catalog, DataType, HashSet, QuerySource};
use crate::catalog::PropertyGraphMeta;
use crate::sql::ast::{
    GraphPathMode, GraphQuantifier, GraphSelectorKind, GraphTable, GraphTableBinding,
    GraphVariableBinding, IdentifierPath, TableFunctionForm,
};

const UNDEFINED_OBJECT: &str = "42704";
const INVALID_PARAMETER_VALUE: &str = "22023";
const DUPLICATE_COLUMN: &str = "42701";

/// The stored graph `name` resolves to through `search_path`.
///
/// # Errors
///
/// `42704` when no graph is visible under the name.
pub(crate) fn resolve_graph(
    catalog: &Catalog,
    context: &BindingContext,
    name: &IdentifierPath,
) -> Result<PropertyGraphMeta, CassieError> {
    let canonical = super::resolve_existing_name(name.as_str(), context, |candidate| {
        catalog.get_property_graph(candidate).is_some()
    })?;
    catalog.get_property_graph(&canonical).ok_or_else(|| {
        diagnostic(
            UNDEFINED_OBJECT,
            format!("property graph \"{}\" does not exist", name.as_str()),
        )
    })
}

/// Canonical names of the tables a pattern can read: every selected mapping and the vertex
/// tables its selected edges connect. A graph never grants access, so each needs the
/// privilege a direct query of the table would need.
///
/// # Errors
///
/// An unknown label is `42704`.
pub(crate) fn tables_read(
    graph: &PropertyGraphMeta,
    table: &GraphTable,
) -> Result<Vec<String>, CassieError> {
    let (_, elements) = scope::collect(graph, table)?;
    Ok(read_tables(graph, &elements))
}

fn read_tables(graph: &PropertyGraphMeta, elements: &[scope::Element<'_>]) -> Vec<String> {
    let mut tables = elements
        .iter()
        .map(|element| element.table.to_owned())
        .collect::<Vec<_>>();
    for alias in elements.iter().flat_map(|element| element.endpoints) {
        tables.extend(
            graph
                .vertices
                .iter()
                .filter(|vertex| vertex.alias == alias)
                .map(|vertex| vertex.table.clone()),
        );
    }
    tables.sort_unstable();
    tables.dedup();
    tables
}

/// The `variable.property` types a table's expressions refer to, for parameter typing
/// before binding. A bound table answers from its binding; otherwise the graph is found by
/// name and a name that is not unique among the visible graphs types nothing.
pub(crate) fn property_types(table: &GraphTable, catalog: &Catalog) -> crate::sql::FieldTypeMap {
    let mut types = crate::sql::FieldTypeMap::new();
    if let Some(binding) = &table.binding {
        for variable in &binding.variables {
            for (property, data_type) in &variable.properties {
                expr::insert_type(
                    &mut types,
                    &format!("{}.{property}", variable.name),
                    data_type,
                );
            }
        }
        return types;
    }
    let wanted = table.graph.as_str().to_ascii_lowercase();
    let mut matches = catalog.list_property_graphs().into_iter().filter(|graph| {
        let name = graph.name.to_ascii_lowercase();
        name == wanted || name.ends_with(&format!(".{wanted}"))
    });
    let (Some(graph), None) = (matches.next(), matches.next()) else {
        return types;
    };
    if let Ok((variables, _)) = scope::collect(&graph, table) {
        for variable in &variables {
            for (property, data_type) in scope::consistent_properties(variable) {
                expr::insert_type(
                    &mut types,
                    &format!("{}.{property}", variable.name),
                    &data_type,
                );
            }
        }
    }
    types
}

/// An unbounded quantifier under `WALK` never terminates on a cycle, with or without a
/// selector. Quantifier bounds and `SHORTEST k` are re-checked here as the parser's
/// guarantees are not the binder's.
fn validate_pattern(table: &GraphTable) -> Result<(), CassieError> {
    let selector = table.effective_selector();
    if let GraphSelectorKind::Shortest(0) = selector.kind {
        return Err(diagnostic(
            INVALID_PARAMETER_VALUE,
            "SHORTEST count must be greater than zero",
        ));
    }
    for step in &table.path.steps {
        match step.edge.quantifier {
            Some(GraphQuantifier::Range { min, max }) if min > max => {
                return Err(diagnostic(
                    INVALID_PARAMETER_VALUE,
                    format!("quantifier lower bound {min} exceeds its upper bound {max}"),
                ));
            }
            Some(GraphQuantifier::ZeroOrMore | GraphQuantifier::OneOrMore)
                if selector.mode == GraphPathMode::Walk =>
            {
                return Err(unsupported(
                    "an unbounded quantifier under WALK is not supported: use a finite bound or \
                     TRAIL, ACYCLIC or SIMPLE",
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

fn check_expressions(
    table: &GraphTable,
    variables: &[Variable<'_>],
    outer_fields: &HashSet<String>,
    path_type: Option<&DataType>,
) -> Result<Vec<DataType>, CassieError> {
    let scope = Scope {
        variables,
        outer_fields,
    };
    let inline = std::iter::once((table.path.start.filter.as_ref(), None)).chain(
        table.path.steps.iter().flat_map(|step| {
            let own = step.edge.quantifier.and(step.edge.variable.as_deref());
            [
                (step.edge.filter.as_ref(), own),
                (step.vertex.filter.as_ref(), None),
            ]
        }),
    );
    for (filter, own_group) in inline {
        if let Some(filter) = filter {
            expr::check(filter, &scope, own_group)?;
            expr::check_predicate(filter, &scope)?;
        }
    }
    if let Some(filter) = &table.filter {
        expr::check(filter, &scope, None)?;
        expr::check_predicate(filter, &scope)?;
    }
    let mut names = HashSet::new();
    let mut output = Vec::with_capacity(table.columns.len());
    for column in &table.columns {
        let name = column.output_name().unwrap_or_default();
        if !names.insert(name.clone()) {
            return Err(diagnostic(
                DUPLICATE_COLUMN,
                format!("column name \"{name}\" appears more than once in GRAPH_TABLE COLUMNS"),
            ));
        }
        if expr::is_path_projection(&column.expr, &scope) {
            output.push(path_type.cloned().ok_or_else(|| {
                diagnostic(
                    UNDEFINED_OBJECT,
                    "type cassie_graph.graph_path does not exist: the reserved schema is not \
                     bootstrapped",
                )
            })?);
            continue;
        }
        expr::check(&column.expr, &scope, None)?;
        output.push(expr::column_type(&column.expr, &scope)?);
    }
    Ok(output)
}

/// Binds a `GRAPH_TABLE` source and records what it resolved.
///
/// # Errors
///
/// Returns the diagnostic of the first failed rule: unknown graph, label or property,
/// incompatible property types, duplicate or conflicting variables, scalar use of a group
/// variable, an unbounded `WALK`, or an unsupported construct.
pub(super) fn bind(
    source: QuerySource,
    catalog: &Catalog,
    lateral_fields: &HashSet<String>,
    context: &BindingContext,
) -> Result<QuerySource, CassieError> {
    let QuerySource::TableFunction {
        name,
        function,
        lateral,
        with_ordinality,
        form: Some(form),
    } = source
    else {
        return Err(diagnostic(
            "42601",
            "GRAPH_TABLE requires a MATCH ... COLUMNS body",
        ));
    };
    let TableFunctionForm::GraphTable(mut table) = *form else {
        return Err(diagnostic("42601", "invalid GRAPH_TABLE call"));
    };
    if with_ordinality {
        return Err(unsupported(
            "WITH ORDINALITY is not supported on GRAPH_TABLE",
        ));
    }
    let graph = resolve_graph(catalog, context, &table.graph)?;
    validate_pattern(&table)?;
    let (variables, elements) = scope::collect(&graph, &table)?;
    let path_type = catalog
        .graph_descriptors(&context.database)
        .map(|descriptors| DataType::Composite(descriptors.path));
    let output = check_expressions(&table, &variables, lateral_fields, path_type.as_ref())?;
    table.binding = Some(GraphTableBinding {
        graph: graph.name.clone(),
        tables: read_tables(&graph, &elements),
        variables: variables
            .iter()
            .map(|variable| GraphVariableBinding {
                name: variable.name.clone(),
                kind: variable.kind,
                group: variable.group,
                mappings: variable
                    .elements
                    .iter()
                    .map(|element| element.mapping_id.to_owned())
                    .collect(),
                properties: scope::consistent_properties(variable),
            })
            .collect(),
        output,
    });
    Ok(QuerySource::TableFunction {
        name,
        function,
        lateral,
        with_ordinality,
        form: Some(Box::new(TableFunctionForm::GraphTable(table))),
    })
}
