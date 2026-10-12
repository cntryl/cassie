//! Private float8 input facts, independent of the output descriptor ABI.
use super::{
    Analyzer, CassieError, Catalog, CommonTableExpression, CteScope, Expr, HashMap, OutputField,
    OutputFields, QuerySource, Schema,
};

#[derive(Default)]
pub(in crate::sql::binder) struct ComparisonTypes {
    lookup: OutputFields,
    ctes: CteScope,
}

impl ComparisonTypes {
    pub(in crate::sql::binder) fn for_scope(
        source: &QuerySource,
        ctes: &[CommonTableExpression],
        schemas: &HashMap<String, Schema>,
        outer: Option<&Self>,
        catalog: &Catalog,
        functions: &HashMap<String, crate::catalog::FunctionMeta>,
        declared_oids: &[i32],
    ) -> Result<Self, CassieError> {
        // Existing binder calls have no controls parameter. This does not
        // invent an unlimited request or claim new binding-resource coverage.
        let analyzer = Analyzer {
            catalog,
            functions,
            declared_oids,
            controls: None,
        };
        let mut origins = schemas
            .iter()
            .map(|(name, schema)| {
                let fields = schema
                    .fields
                    .iter()
                    .cloned()
                    .map(|field| {
                        let mut output = OutputField::from_schema(field);
                        // A schema-only inherited derived field is not a declaration.
                        output.value.proven_float8 = false;
                        output
                    })
                    .collect();
                (name.clone(), fields)
            })
            .collect::<CteScope>();
        if let Some(outer) = outer {
            origins.extend(outer.ctes.clone());
        }
        for cte in ctes {
            let fields = analyzer.cte(cte, &origins)?;
            origins.insert(cte.name.to_ascii_lowercase(), fields);
        }
        let lookup = analyzer
            .source(source, &origins, outer.map(|scope| scope.lookup.as_slice()))?
            .lookup;
        Ok(Self {
            lookup,
            ctes: origins,
        })
    }

    pub(in crate::sql::binder) fn expression(
        &self,
        expr: &Expr,
        catalog: &Catalog,
        functions: &HashMap<String, crate::catalog::FunctionMeta>,
        declared_oids: &[i32],
    ) -> bool {
        Analyzer {
            catalog,
            functions,
            declared_oids,
            controls: None,
        }
        .expression(expr, &self.lookup)
        .is_ok_and(|output| output.proven_float8)
    }

    pub(in crate::sql::binder) fn extend_outer(&mut self, outer: &Self) {
        self.lookup.extend_from_slice(&outer.lookup);
    }

    pub(in crate::sql::binder) fn extend_excluded(&mut self, fields: &[crate::types::FieldSchema]) {
        self.lookup
            .extend(fields.iter().cloned().map(OutputField::from_schema));
    }
}
