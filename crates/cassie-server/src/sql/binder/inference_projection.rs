//! Field names and type evidence for ordinary SELECT outputs and set operands.
use super::{
    infer_expr_type, infer_function_return_type, schema_field_type, DataType, Expr, FieldSchema,
    HashMap, ProjectionMode, Schema, SelectItem,
};
use crate::sql::binder::infer_window_return_type;

pub(super) fn projection_schema(
    projection: &[SelectItem],
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
    mode: ProjectionMode,
) -> Schema {
    let mut fields = Vec::new();
    for item in projection {
        match item {
            SelectItem::Wildcard => fields.extend(source_schema.fields.iter().cloned()),
            SelectItem::Column { name, alias } => {
                let output_name = alias.clone().unwrap_or_else(|| {
                    crate::sql::ColumnIdentifierPath::parse(name)
                        .map_or_else(|_| name.clone(), |column| column.implicit_output_label())
                });
                fields.push(FieldSchema {
                    name: output_name,
                    data_type: schema_field_type(source_schema, name)
                        .unwrap_or_else(|| mode.unresolved()),
                    nullable: true,
                });
            }
            SelectItem::Function { function, alias } => {
                let output_name = alias
                    .as_deref()
                    .unwrap_or(function.name.as_str())
                    .to_string();
                fields.push(FieldSchema {
                    name: output_name,
                    data_type: infer_function_return_type(
                        function,
                        source_schema,
                        user_functions,
                        parameter_types,
                    )
                    .unwrap_or_else(|| mode.unresolved()),
                    nullable: true,
                });
            }
            SelectItem::Expr { expr, alias } => {
                fields.push(FieldSchema {
                    name: alias.as_deref().unwrap_or("expr").to_string(),
                    data_type: if mode == ProjectionMode::SetOperand
                        && matches!(expr, Expr::StringLiteral(_))
                    {
                        DataType::Null
                    } else {
                        infer_expr_type(expr, source_schema, user_functions, parameter_types)
                            .unwrap_or_else(|| mode.unresolved())
                    },
                    nullable: true,
                });
            }
            SelectItem::WindowFunction { function, alias } => {
                let data_type = infer_window_return_type(
                    function,
                    source_schema,
                    user_functions,
                    parameter_types,
                );
                fields.push(FieldSchema {
                    name: alias
                        .as_deref()
                        .unwrap_or(function.name.as_str())
                        .to_string(),
                    data_type,
                    nullable: false,
                });
            }
        }
    }

    Schema { fields }
}
