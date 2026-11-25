//! Search scoring uses corpus statistics. Snippets need only field analyzers,
//! except when a query asks to expand a prefix or typo against a field's vocabulary.
use super::super::fulltext_options::fulltext_base_collection;
use super::plan_inspection::SearchFields;
use super::{
    filter, load_fulltext_index_options, AnalyzerConfig, Batch, Cassie, HashMap, HashSet,
    LogicalPlan, QueryError, QuerySource,
};
use crate::runtime::QueryMemoryReservation;

pub(super) fn snippet_context(
    cassie: &Cassie,
    plan: &LogicalPlan,
    fields: &SearchFields,
) -> Result<Option<filter::SearchContext>, QueryError> {
    if fields.snippets.is_empty() {
        return Ok(None);
    }
    let Some(collection) = fulltext_base_collection(&plan.source, &plan.ctes) else {
        return Ok(None);
    };
    let options = load_fulltext_index_options(cassie, collection, &fields.snippets)?;
    Ok((!options.field_analyzer.is_empty())
        .then(|| filter::SearchContext::with_analyzers(options.field_analyzer)))
}

type SearchContextWithMemory = (
    Option<filter::SearchContext>,
    Option<QueryMemoryReservation>,
);

pub(super) fn build(
    env: &super::SourceExecutionEnv<'_>,
    plan: &LogicalPlan,
    batches: &[Batch],
    text_fields: &[String],
    fields: &SearchFields,
) -> Result<SearchContextWithMemory, QueryError> {
    let cassie = env.cassie;
    if fields.scored.is_empty() {
        let context = snippet_context(cassie, plan, fields)?;
        if fields.expanded.is_empty() {
            return Ok((context, None));
        }
        let mut context = context.unwrap_or_default();
        let mut memory = env.controls.reserve_query_memory(0)?;
        for field in &fields.expanded {
            let texts = batches
                .iter()
                .flatten()
                .filter_map(|row| row.get(field)?.as_str());
            let analyzer = context.analyzer_for_field(field);
            for text in texts {
                env.controls.check_interruption()?;
                let _scratch = env.controls.reserve_query_memory(
                    super::super::scored::tokenized_text_upper_bound(text, &analyzer),
                )?;
                context.add_snippet_terms(field, analyzer.analyze(text), &mut memory)?;
            }
        }
        return Ok((Some(context), Some(memory)));
    }
    let requested = fields.scored.union(&fields.snippets).cloned().collect();
    let options = search_context_options(cassie, &plan.source, &plan.ctes, &requested)?;
    cassie
        .runtime
        .record_fulltext_row_scan_fallback("authoritative_row_scan");
    Ok((
        Some(filter::SearchContext::from_rows(
            batches.iter().flatten(),
            text_fields,
            &options.boost,
            &options.k1,
            &options.b,
            &options.analyzer,
        )),
        None,
    ))
}

struct SearchContextOptions {
    boost: HashMap<String, f64>,
    k1: HashMap<String, f64>,
    b: HashMap<String, f64>,
    analyzer: HashMap<String, AnalyzerConfig>,
}

fn search_context_options(
    cassie: &Cassie,
    source: &QuerySource,
    ctes: &[crate::sql::ast::CommonTableExpression],
    fulltext_fields: &HashSet<String>,
) -> Result<SearchContextOptions, QueryError> {
    // A derived table or CTE that passes a base table's columns through
    // unrenamed searches with that table's fulltext index options.
    let Some(name) = fulltext_base_collection(source, ctes) else {
        return Ok(SearchContextOptions {
            boost: HashMap::new(),
            k1: HashMap::new(),
            b: HashMap::new(),
            analyzer: HashMap::new(),
        });
    };

    let mut field_boost = HashMap::with_capacity(cassie.catalog.text_fields(name).len());
    for field in cassie.catalog.text_fields(name) {
        if let Some(value) = cassie.catalog.get_field_boost(name, &field) {
            field_boost.insert(
                crate::sql::ColumnIdentifierPath::stored_field_key(&field),
                f64::from(value),
            );
        }
    }

    let index_options = load_fulltext_index_options(cassie, name, fulltext_fields)?;
    for (field, value) in index_options.field_boost {
        field_boost.insert(field, value);
    }

    Ok(SearchContextOptions {
        boost: field_boost,
        k1: index_options.field_k1,
        b: index_options.field_b,
        analyzer: index_options.field_analyzer,
    })
}
