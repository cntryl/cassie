//! Resolve type-function notation without hiding exact catalog routine signatures.
use super::{coalesce_results::ResultTypes, CassieError, DataType, Expr, FunctionCall, SelectItem};
use crate::sql::ast::CastIdentity;
use crate::sql::functions::primitive_casts;

pub(super) fn lower(
    function: &mut FunctionCall,
    types: &ResultTypes,
) -> Result<Option<Expr>, CassieError> {
    let Some(target) = primitive_casts::candidate(function) else {
        return Ok(None);
    };
    let argument = &function.args[0];
    let unknown = primitive_casts::unknown_literal(argument);
    if !unknown {
        if let Expr::Param(index) = argument {
            if matches!(
                types.parameter_types().get(*index).copied(),
                None | Some(0 | 705)
            ) {
                return Ok(None);
            }
        }
        let Some(source) = types.expression_type(argument) else {
            return Ok(None);
        };
        if types
            .declared_function(&function.name)
            .is_some_and(|metadata| {
                metadata.args.len() == 1 && metadata.args[0].data_type == source
            })
        {
            return Ok(None);
        }
        if matches!(
            source,
            DataType::Array(_) | DataType::Json | DataType::Vector(_)
        ) {
            return Err(CassieError::Unsupported(
                "array, JSON and vector type-function conversions remain pending under #807/#809/#793".into(),
            ));
        }
        if crate::sql::functions::has_unselected_numeric(argument) {
            return Err(CassieError::Unsupported(
                "NUMERIC type-function conversions remain pending under #793/#812".into(),
            ));
        }
    }
    Ok(Some(Expr::Cast {
        expr: Box::new(function.args.pop().expect("one admitted cast argument")),
        data_type: target.data_type,
        source_type: None,
        identity: CastIdentity::Scalar,
    }))
}

pub(super) fn lower_expression(
    expression: &mut Expr,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    if let Expr::Function(function) = expression {
        if let Some(cast) = lower(function, types)? {
            *expression = cast;
        }
    }
    Ok(())
}

pub(super) fn lower_item(item: &mut SelectItem, types: &ResultTypes) -> Result<(), CassieError> {
    let SelectItem::Function { function, alias } = item else {
        return Ok(());
    };
    if let Some(expr) = lower(function, types)? {
        let alias = alias.take().or_else(|| {
            Some(
                function
                    .name
                    .rsplit('.')
                    .next()
                    .unwrap_or(&function.name)
                    .to_ascii_lowercase(),
            )
        });
        *item = SelectItem::Expr { expr, alias };
    }
    Ok(())
}
