//! Expressions inside a `GRAPH_TABLE`: reference scoping and typing (#1256).

use super::graph_table_scope::{
    consistent_properties, diagnostic, property_type, unsupported, Variable,
};
use super::{CassieError, DataType, Expr, HashMap, HashSet, Schema};
use crate::sql::ast::GraphVariableKind;
use crate::sql::{ColumnIdentifierPath, FieldTypeMap};
use crate::types::FieldSchema;

const UNDEFINED_COLUMN: &str = "42703";
const SYNTAX_ERROR: &str = "42601";
const DATATYPE_MISMATCH: &str = "42804";
const INDETERMINATE_DATATYPE: &str = "42P18";

/// What a pattern expression may refer to.
pub(super) struct Scope<'a, 'g> {
    pub(super) variables: &'a [Variable<'g>],
    /// Columns of preceding `FROM` items, visible when the source is `LATERAL`.
    pub(super) outer_fields: &'a HashSet<String>,
}

impl Scope<'_, '_> {
    fn variable(&self, name: &str) -> Option<&Variable<'_>> {
        self.variables.iter().find(|variable| variable.name == name)
    }

    /// Property types of every variable, keyed the way column references look up.
    fn schema(&self) -> (Schema, FieldTypeMap) {
        let mut fields = Vec::new();
        let mut types = FieldTypeMap::new();
        for variable in self.variables {
            for (property, data_type) in consistent_properties(variable) {
                let name = property_name(&variable.name, &property);
                insert_type(&mut types, &name, &data_type);
                fields.push(FieldSchema {
                    name,
                    data_type,
                    nullable: true,
                });
            }
        }
        (Schema { fields }, types)
    }
}

pub(super) fn property_name(variable: &str, property: &str) -> String {
    format!(
        "{}.{}",
        crate::catalog::canonical_identifier_component(variable),
        crate::catalog::canonical_identifier_component(property)
    )
}

/// Registers `name` (`variable.property`) the way column references look it up.
pub(super) fn insert_type(types: &mut FieldTypeMap, name: &str, data_type: &DataType) {
    if let Ok(path) = ColumnIdentifierPath::parse(name) {
        types.insert(path.namespace_key(), data_type.clone());
        types.insert(path.lookup_key(), data_type.clone());
    }
}

/// A path variable is a `cassie_graph.graph_path` value. The contract defines only its
/// projection: it is not a scalar, has no properties and takes part in no other expression.
fn path_valued_use(variable: &str) -> CassieError {
    unsupported(format!(
        "path variable \"{variable}\" can only be projected directly as a COLUMNS item, \
         not used inside an expression or predicate"
    ))
}

/// Whether `expr` is exactly a reference to the path variable of the pattern: the one
/// expression that projects a `cassie_graph.graph_path` value.
pub(super) fn is_path_projection(expr: &Expr, scope: &Scope<'_, '_>) -> bool {
    let Expr::Column(name) = expr else {
        return false;
    };
    ColumnIdentifierPath::parse(name).is_ok_and(|path| {
        !path.is_qualified()
            && scope
                .variable(path.final_name())
                .is_some_and(|variable| variable.kind == GraphVariableKind::Path)
    })
}

/// Checks every reference of `expr`. `own_group` names the quantified edge variable whose
/// inline predicate `expr` is, the only place a group variable's properties are in scope.
pub(super) fn check(
    expr: &Expr,
    scope: &Scope<'_, '_>,
    own_group: Option<&str>,
) -> Result<(), CassieError> {
    match expr {
        Expr::Column(name) => check_reference(name, scope, own_group),
        Expr::Exists(_) | Expr::Subquery(_) => Err(unsupported(
            "subqueries are not supported inside GRAPH_TABLE",
        )),
        Expr::Function(function)
            if crate::sql::functions::is_aggregate_function(&function.name) =>
        {
            Err(unsupported(format!(
                "aggregate function {} is not supported inside GRAPH_TABLE",
                function.name
            )))
        }
        _ => expr.try_visit_children(|child| check(child, scope, own_group)),
    }
}

fn check_reference(
    name: &str,
    scope: &Scope<'_, '_>,
    own_group: Option<&str>,
) -> Result<(), CassieError> {
    let path =
        ColumnIdentifierPath::parse(name).map_err(|message| diagnostic(SYNTAX_ERROR, message))?;
    if path.is_qualified() {
        if let Some(variable) = path.namespace_qualifier().and_then(|q| scope.variable(&q)) {
            return check_property(variable, &path.declared_name(), own_group);
        }
    } else if let Some(variable) = scope.variable(path.final_name()) {
        return Err(bare_variable(variable));
    }
    if scope.outer_fields.contains(&path.lookup_key())
        || scope.outer_fields.contains(&path.namespace_key())
    {
        return Ok(());
    }
    Err(diagnostic(
        UNDEFINED_COLUMN,
        format!("column or variable \"{name}\" does not exist"),
    ))
}

fn check_property(
    variable: &Variable<'_>,
    property: &str,
    own_group: Option<&str>,
) -> Result<(), CassieError> {
    if variable.kind == GraphVariableKind::Path {
        return Err(diagnostic(
            UNDEFINED_COLUMN,
            format!("path variable \"{}\" has no properties", variable.name),
        ));
    }
    if variable.group && own_group != Some(variable.name.as_str()) {
        return Err(unsupported(format!(
            "direct scalar use of the group variable \"{}\" is not supported",
            variable.name
        )));
    }
    property_type(variable, property).map(|_| ())
}

fn bare_variable(variable: &Variable<'_>) -> CassieError {
    match variable.kind {
        GraphVariableKind::Path => path_valued_use(&variable.name),
        GraphVariableKind::Vertex | GraphVariableKind::Edge if variable.group => {
            unsupported(format!(
                "direct scalar use of the group variable \"{}\" is not supported",
                variable.name
            ))
        }
        _ => unsupported(format!(
            "element variable \"{}\" cannot be projected; project its properties",
            variable.name
        )),
    }
}

/// Requires a predicate to be boolean and consistent in its operand families.
pub(super) fn check_predicate(expr: &Expr, scope: &Scope<'_, '_>) -> Result<(), CassieError> {
    let (schema, types) = scope.schema();
    super::validation::validate_expression_operand_families(expr, &types)?;
    match super::inference::infer_expr_type(expr, &schema, &HashMap::new(), &[]) {
        Some(DataType::Boolean | DataType::Null) | None => Ok(()),
        Some(_) => Err(diagnostic(
            DATATYPE_MISMATCH,
            "argument of GRAPH_TABLE WHERE must be type boolean",
        )),
    }
}

/// The type of a `COLUMNS` projection.
pub(super) fn column_type(expr: &Expr, scope: &Scope<'_, '_>) -> Result<DataType, CassieError> {
    let (schema, types) = scope.schema();
    super::validation::validate_expression_operand_families(expr, &types)?;
    match super::inference::infer_expr_type(expr, &schema, &HashMap::new(), &[]) {
        Some(DataType::Null) => Ok(DataType::Text),
        Some(data_type) => Ok(data_type.flattened().clone()),
        None => Err(diagnostic(
            INDETERMINATE_DATATYPE,
            "could not determine the data type of a GRAPH_TABLE column",
        )),
    }
}
