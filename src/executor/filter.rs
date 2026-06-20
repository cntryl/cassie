use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use serde::{Deserialize, Serialize};

use crate::app::CassieSession;
use crate::catalog::FunctionMeta;
use crate::executor::batch::{Batch, RowAccess};
use crate::executor::QueryError;
use crate::sql::ast::FunctionCall;
use crate::sql::ast::{BinaryOp, Expr};
use crate::types::{DataType, Value};
use uuid::Uuid;

thread_local! {
    static FUNCTION_CACHE: RefCell<FunctionCache> = RefCell::new(FunctionCache::new());
}

const FUNCTION_CACHE_SIZE: usize = 256;

struct FunctionCache {
    keys: Vec<u64>,
    values: Vec<Value>,
}

impl FunctionCache {
    fn new() -> Self {
        Self {
            keys: Vec::with_capacity(FUNCTION_CACHE_SIZE),
            values: Vec::with_capacity(FUNCTION_CACHE_SIZE),
        }
    }

    fn lookup(&self, key: u64) -> Option<&Value> {
        self.keys
            .iter()
            .position(|k| *k == key)
            .and_then(|i| self.values.get(i))
    }

    fn store(&mut self, key: u64, value: Value) {
        if let Some(pos) = self.keys.iter().position(|k| *k == key) {
            self.values[pos] = value;
            return;
        }
        if self.keys.len() >= FUNCTION_CACHE_SIZE {
            self.keys.remove(0);
            self.values.remove(0);
        }
        self.keys.push(key);
        self.values.push(value);
    }
}

fn function_cache_key(name: &str, args: &[Value]) -> u64 {
    use std::hash::Hasher;
    fn hash_value(hasher: &mut std::hash::DefaultHasher, value: &Value) {
        match value {
            Value::Null => 0u8.hash(hasher),
            Value::Bool(v) => {
                1u8.hash(hasher);
                v.hash(hasher);
            }
            Value::Int64(v) => {
                2u8.hash(hasher);
                v.hash(hasher);
            }
            Value::Float64(v) => {
                3u8.hash(hasher);
                v.to_bits().hash(hasher);
            }
            Value::String(v) => {
                4u8.hash(hasher);
                v.hash(hasher);
            }
            Value::Vector(v) => {
                5u8.hash(hasher);
                v.values.len().hash(hasher);
            }
            Value::Json(v) => {
                6u8.hash(hasher);
                v.to_string().hash(hasher);
            }
        }
    }
    let mut hasher = std::hash::DefaultHasher::new();
    name.hash(&mut hasher);
    for arg in args {
        hash_value(&mut hasher, arg);
    }
    hasher.finish()
}

fn has_only_constant_args(exprs: &[Expr]) -> bool {
    exprs.iter().all(is_constant_expr)
}

fn is_constant_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Null
        | Expr::BoolLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::StringLiteral(_)
        | Expr::Param(_) => true,
        Expr::Function(f) => f.args.iter().all(is_constant_expr),
        Expr::Binary { left, right, .. } => is_constant_expr(left) && is_constant_expr(right),
        Expr::Cast { expr, .. } => is_constant_expr(expr),
        Expr::InList { expr, values, .. } => {
            is_constant_expr(expr) && values.iter().all(is_constant_expr)
        }
        Expr::Not { expr } => is_constant_expr(expr),
        Expr::IsNull { expr, .. } => is_constant_expr(expr),
        Expr::Between {
            expr, low, high, ..
        } => is_constant_expr(expr) && is_constant_expr(low) && is_constant_expr(high),
        _ => false,
    }
}

#[derive(Debug, Clone)]
pub(crate) enum ScalarValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct SearchContext {
    total_documents: usize,
    doc_frequency: HashMap<String, HashMap<String, usize>>,
    avg_doc_length: HashMap<String, f64>,
    doc_boost: HashMap<String, f64>,
    field_k1: HashMap<String, f64>,
    field_b: HashMap<String, f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SearchTermStats {
    has_text: bool,
    doc_length: usize,
    term_counts: HashMap<String, usize>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SingleFieldSearchContext {
    total_documents: usize,
    doc_frequency: HashMap<String, usize>,
    avg_doc_length: f64,
    boost: f64,
    k1: f64,
    b: f64,
}

impl SearchTermStats {
    pub(crate) fn from_text(source: Option<&str>) -> Self {
        let Some(source) = source else {
            return Self::default();
        };
        let tokens = crate::search::tokenizer::tokenize(source);
        Self {
            has_text: true,
            doc_length: tokens.len(),
            term_counts: token_counts(tokens.as_slice()),
        }
    }

    pub(crate) fn term_counts(&self) -> &HashMap<String, usize> {
        &self.term_counts
    }
}

impl SingleFieldSearchContext {
    pub(crate) fn from_term_stats<'a, I>(
        field: &str,
        documents: I,
        field_boost: &HashMap<String, f64>,
        field_k1: &HashMap<String, f64>,
        field_b: &HashMap<String, f64>,
    ) -> Self
    where
        I: IntoIterator<Item = &'a SearchTermStats>,
    {
        let field = field.to_ascii_lowercase();
        let mut context = Self {
            boost: field_boost.get(&field).copied().unwrap_or(1.0),
            k1: field_k1
                .get(&field)
                .copied()
                .unwrap_or(crate::search::bm25::DEFAULT_BM25_K1),
            b: field_b
                .get(&field)
                .copied()
                .unwrap_or(crate::search::bm25::DEFAULT_BM25_B),
            ..Default::default()
        };
        let mut docs_with_field = 0usize;
        let mut length_sum = 0usize;

        for stats in documents {
            context.total_documents += 1;
            if !stats.has_text {
                continue;
            }

            docs_with_field += 1;
            length_sum += stats.doc_length;
            for term in stats.term_counts.keys() {
                context
                    .doc_frequency
                    .entry(term.clone())
                    .and_modify(|count| *count += 1)
                    .or_insert(1);
            }
        }

        if docs_with_field > 0 {
            context.avg_doc_length = length_sum as f64 / docs_with_field as f64;
        }

        context
    }

    pub(crate) fn score_term_stats(
        &self,
        source_stats: &SearchTermStats,
        query_terms: &[String],
    ) -> f64 {
        if query_terms.is_empty() || !source_stats.has_text || source_stats.doc_length == 0 {
            return 0.0;
        }

        let dl = source_stats.doc_length as f64;
        let docs = self.total_documents.max(1) as f64;
        let avg_dl = if self.avg_doc_length > 0.0 {
            self.avg_doc_length
        } else {
            dl
        };

        let mut score = 0.0;
        for term in query_terms {
            let tf = source_stats.term_counts.get(term).copied().unwrap_or(0) as f64;
            if tf == 0.0 {
                continue;
            }

            let df = self.doc_frequency.get(term).copied().unwrap_or(0) as f64;
            score += crate::search::bm25_score(tf, df, docs, self.k1, self.b, dl, avg_dl);
        }

        score * self.boost
    }
}

impl SearchContext {
    pub(crate) fn from_rows<'a, I, R>(
        rows: I,
        text_fields: &[String],
        field_boost: &HashMap<String, f64>,
        field_k1: &HashMap<String, f64>,
        field_b: &HashMap<String, f64>,
    ) -> Self
    where
        I: IntoIterator<Item = &'a R>,
        R: RowAccess + 'a,
    {
        let mut context = Self {
            doc_boost: field_boost.clone(),
            field_k1: field_k1.clone(),
            field_b: field_b.clone(),
            ..Default::default()
        };

        let text_fields = text_fields
            .iter()
            .map(|field| field.to_lowercase())
            .collect::<HashSet<_>>();
        let mut term_occurrence = HashMap::<String, usize>::new();
        let mut text_length = HashMap::<String, usize>::new();

        for row in rows {
            context.total_documents += 1;
            for (name, value) in row.entries() {
                let name = name.to_lowercase();
                if !text_fields.is_empty() && !text_fields.contains(&name) {
                    continue;
                }

                let Value::String(text) = value else {
                    continue;
                };
                let term_stats = SearchTermStats::from_text(Some(text));
                text_length
                    .entry(name.clone())
                    .and_modify(|value| *value += term_stats.doc_length)
                    .or_insert(term_stats.doc_length);
                *term_occurrence.entry(name.clone()).or_insert(0) += 1;
                for term in term_stats.term_counts.keys() {
                    context
                        .doc_frequency
                        .entry(name.clone())
                        .or_default()
                        .entry(term.clone())
                        .and_modify(|count| *count += 1)
                        .or_insert(1);
                }
            }
        }

        for (name, length_sum) in text_length {
            let docs_with_field = *term_occurrence.get(&name).unwrap_or(&1) as f64;
            if docs_with_field > 0.0 {
                context
                    .avg_doc_length
                    .insert(name, length_sum as f64 / docs_with_field);
            }
        }

        context
    }

    pub(crate) fn total_documents(&self) -> usize {
        self.total_documents
    }

    #[allow(dead_code)]
    pub(crate) fn from_term_stats<'a, I>(
        field: &str,
        documents: I,
        field_boost: &HashMap<String, f64>,
        field_k1: &HashMap<String, f64>,
        field_b: &HashMap<String, f64>,
    ) -> Self
    where
        I: IntoIterator<Item = &'a SearchTermStats>,
    {
        let field = field.to_lowercase();
        let mut context = Self {
            doc_boost: field_boost.clone(),
            field_k1: field_k1.clone(),
            field_b: field_b.clone(),
            ..Default::default()
        };
        let mut docs_with_field = 0usize;
        let mut length_sum = 0usize;

        for stats in documents {
            context.total_documents += 1;
            if !stats.has_text {
                continue;
            }

            docs_with_field += 1;
            length_sum += stats.doc_length;
            for term in stats.term_counts.keys() {
                context
                    .doc_frequency
                    .entry(field.clone())
                    .or_default()
                    .entry(term.clone())
                    .and_modify(|count| *count += 1)
                    .or_insert(1);
            }
        }

        if docs_with_field > 0 {
            context
                .avg_doc_length
                .insert(field, length_sum as f64 / docs_with_field as f64);
        }

        context
    }

    fn average_doc_length(&self, field: &str) -> Option<f64> {
        self.avg_doc_length.get(&field.to_lowercase()).copied()
    }

    fn document_frequency(&self, field: &str, term: &str) -> Option<usize> {
        self.doc_frequency
            .get(&field.to_lowercase())
            .and_then(|terms| terms.get(&term.to_lowercase()).copied())
    }

    fn field_boost(&self, field: &str) -> f64 {
        self.doc_boost
            .get(&field.to_lowercase())
            .copied()
            .unwrap_or(1.0)
    }

    fn field_k1(&self, field: &str) -> f64 {
        self.field_k1
            .get(&field.to_lowercase())
            .copied()
            .unwrap_or(crate::search::bm25::DEFAULT_BM25_K1)
    }

    fn field_b(&self, field: &str) -> f64 {
        self.field_b
            .get(&field.to_lowercase())
            .copied()
            .unwrap_or(crate::search::bm25::DEFAULT_BM25_B)
    }

    pub(crate) fn score_text(&self, field: Option<&str>, source: &str, query: &str) -> f64 {
        let query_terms = prepare_query_terms(query);
        let source_stats = SearchTermStats::from_text(Some(source));
        self.score_term_stats(field, &source_stats, &query_terms)
    }

    pub(crate) fn score_term_stats(
        &self,
        field: Option<&str>,
        source_stats: &SearchTermStats,
        query_terms: &[String],
    ) -> f64 {
        if query_terms.is_empty() || !source_stats.has_text || source_stats.doc_length == 0 {
            return 0.0;
        }
        let dl = source_stats.doc_length as f64;

        let docs = self.total_documents.max(1) as f64;
        let field = field.map(|field| field.to_lowercase());
        let avg_dl = field
            .as_deref()
            .and_then(|field| self.average_doc_length(field))
            .unwrap_or(dl);
        let boost = field
            .as_deref()
            .map(|field| self.field_boost(field))
            .unwrap_or(1.0);
        let (k1, b) = field
            .as_deref()
            .map(|field| (self.field_k1(field), self.field_b(field)))
            .unwrap_or((
                crate::search::bm25::DEFAULT_BM25_K1,
                crate::search::bm25::DEFAULT_BM25_B,
            ));

        let mut score = 0.0;
        for term in query_terms {
            let tf = source_stats.term_counts.get(term).copied().unwrap_or(0) as f64;
            if tf == 0.0 {
                continue;
            }

            let df = field
                .as_deref()
                .and_then(|field| self.document_frequency(field, term))
                .unwrap_or(0) as f64;
            score += crate::search::bm25_score(tf, df, docs, k1, b, dl, avg_dl);
        }

        score * boost
    }
}

impl ScalarValue {
    pub(crate) fn as_bool(&self) -> bool {
        match self {
            ScalarValue::Bool(v) => *v,
            ScalarValue::Int(v) => *v != 0,
            ScalarValue::Float(v) => *v != 0.0,
            ScalarValue::Str(v) => !v.is_empty(),
            ScalarValue::Null => false,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            ScalarValue::Str(v) => Some(v),
            _ => None,
        }
    }

    pub(crate) fn to_f64(&self) -> Option<f64> {
        match self {
            ScalarValue::Float(v) => Some(*v),
            ScalarValue::Int(v) => Some(*v as f64),
            ScalarValue::Bool(v) => Some(if *v { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    fn to_value(&self) -> Value {
        match self {
            ScalarValue::Null => Value::Null,
            ScalarValue::Bool(v) => Value::Bool(*v),
            ScalarValue::Int(v) => Value::Int64(*v),
            ScalarValue::Float(v) => Value::Float64(*v),
            ScalarValue::Str(v) => Value::String(v.clone()),
        }
    }
}

pub(crate) fn filter_rows<R>(
    rows: Vec<R>,
    expression: &Expr,
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Vec<R>, QueryError>
where
    R: RowAccess,
{
    Ok(rows
        .into_iter()
        .filter(|row| {
            eval_filter(
                row,
                expression,
                params,
                search_context,
                user_functions,
                session,
            )
            .unwrap_or(false)
        })
        .collect())
}

pub(crate) fn filter_batches(
    batches: Vec<Batch>,
    expression: &Expr,
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Vec<Batch>, QueryError> {
    batches
        .into_iter()
        .map(|batch| {
            filter_rows(
                batch,
                expression,
                params,
                search_context,
                user_functions,
                session,
            )
        })
        .collect()
}

pub(crate) fn evaluate_expr_value<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    local_args: Option<&HashMap<String, Value>>,
) -> Result<Value, QueryError> {
    match expr {
        Expr::Function(function) => evaluate_function(
            function,
            row,
            params,
            search_context,
            user_functions,
            session,
            local_args,
        ),
        _ => Ok(eval_scalar(
            row,
            expr,
            params,
            search_context,
            user_functions,
            None,
            session,
        )?
        .to_value()),
    }
}

fn eval_filter<R: RowAccess + ?Sized>(
    row: &R,
    expression: &Expr,
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<bool, QueryError> {
    let value = eval_scalar(
        row,
        expression,
        params,
        search_context,
        user_functions,
        None,
        session,
    )?;
    Ok(value.as_bool())
}

pub(crate) fn eval_scalar<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    local_args: Option<&HashMap<String, Value>>,
    session: Option<&CassieSession>,
) -> Result<ScalarValue, QueryError> {
    match expr {
        Expr::Column(name) => {
            if let Some(local_args) = local_args {
                let key = name.to_ascii_lowercase();
                if let Some(value) = local_args.get(&key) {
                    return Ok(scalar_from_value(value).unwrap_or(ScalarValue::Null));
                }
            }

            Ok(row
                .get(name)
                .and_then(scalar_from_value)
                .unwrap_or(ScalarValue::Null))
        }
        Expr::StringLiteral(value) => Ok(ScalarValue::Str(value.clone())),
        Expr::NumberLiteral(value) => Ok(ScalarValue::Float(*value)),
        Expr::BoolLiteral(value) => Ok(ScalarValue::Bool(*value)),
        Expr::Null => Ok(ScalarValue::Null),
        Expr::Param(index) => params
            .get(*index)
            .and_then(scalar_from_value)
            .map(Ok)
            .unwrap_or_else(|| Ok(ScalarValue::Null)),
        Expr::Function(function) => Ok(scalar_from_value(&evaluate_function(
            function,
            row,
            params,
            search_context,
            user_functions,
            session,
            local_args,
        )?)
        .unwrap_or(ScalarValue::Null)),
        Expr::Binary { left, op, right } => Ok(binary_scalar(
            &eval_scalar(
                row,
                left,
                params,
                search_context,
                user_functions,
                local_args,
                session,
            )?,
            op,
            &eval_scalar(
                row,
                right,
                params,
                search_context,
                user_functions,
                local_args,
                session,
            )?,
        )),
        Expr::IsNull { expr, negated } => {
            let value = eval_scalar(
                row,
                expr,
                params,
                search_context,
                user_functions,
                local_args,
                session,
            )?;
            let is_null = matches!(value, ScalarValue::Null);
            Ok(ScalarValue::Bool(if *negated { !is_null } else { is_null }))
        }
        Expr::InList {
            expr,
            values,
            negated,
        } => {
            let left = eval_scalar(
                row,
                expr,
                params,
                search_context,
                user_functions,
                local_args,
                session,
            )?;
            let contains = values
                .iter()
                .map(|value| {
                    eval_scalar(
                        row,
                        value,
                        params,
                        search_context,
                        user_functions,
                        local_args,
                        session,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?
                .iter()
                .any(|right| eq_value(&left, right));
            Ok(ScalarValue::Bool(if *negated {
                !contains
            } else {
                contains
            }))
        }
        Expr::Between {
            expr,
            low,
            high,
            negated,
        } => {
            let value = eval_scalar(
                row,
                expr,
                params,
                search_context,
                user_functions,
                local_args,
                session,
            )?;
            let low = eval_scalar(
                row,
                low,
                params,
                search_context,
                user_functions,
                local_args,
                session,
            )?;
            let high = eval_scalar(
                row,
                high,
                params,
                search_context,
                user_functions,
                local_args,
                session,
            )?;
            let in_range = number_cmp(&value, &low, |left, right| left >= right)
                && number_cmp(&value, &high, |left, right| left <= right);
            Ok(ScalarValue::Bool(if *negated {
                !in_range
            } else {
                in_range
            }))
        }
        Expr::Not { expr } => {
            let value = eval_scalar(
                row,
                expr,
                params,
                search_context,
                user_functions,
                local_args,
                session,
            )?;
            Ok(ScalarValue::Bool(!value.as_bool()))
        }
        Expr::Cast { expr, data_type } => {
            let value = eval_scalar(
                row,
                expr,
                params,
                search_context,
                user_functions,
                local_args,
                session,
            )?;
            cast_scalar(&value, data_type)
        }
        Expr::Exists(_) => Err(QueryError::General(
            "EXISTS predicate was not resolved before filtering".to_string(),
        )),
    }
}

fn cast_scalar(value: &ScalarValue, data_type: &DataType) -> Result<ScalarValue, QueryError> {
    if matches!(value, ScalarValue::Null) {
        return Ok(ScalarValue::Null);
    }

    match data_type {
        DataType::Null => Ok(ScalarValue::Null),
        DataType::SmallInt => scalar_to_i64(value)
            .and_then(|value| i16::try_from(value).ok())
            .map(|value| ScalarValue::Int(value as i64))
            .ok_or_else(|| QueryError::General("cannot cast value to SMALLINT".to_string())),
        DataType::Int => scalar_to_i64(value)
            .and_then(|value| i32::try_from(value).ok())
            .map(|value| ScalarValue::Int(value as i64))
            .ok_or_else(|| QueryError::General("cannot cast value to INT".to_string())),
        DataType::BigInt => scalar_to_i64(value)
            .map(ScalarValue::Int)
            .ok_or_else(|| QueryError::General("cannot cast value to BIGINT".to_string())),
        DataType::Float => value
            .to_f64()
            .map(ScalarValue::Float)
            .or_else(|| {
                value
                    .as_str()
                    .and_then(|value| value.parse::<f64>().ok())
                    .map(ScalarValue::Float)
            })
            .ok_or_else(|| QueryError::General("cannot cast value to FLOAT".to_string())),
        DataType::Boolean => match value {
            ScalarValue::Bool(value) => Ok(ScalarValue::Bool(*value)),
            ScalarValue::Int(value) => Ok(ScalarValue::Bool(*value != 0)),
            ScalarValue::Float(value) => Ok(ScalarValue::Bool(*value != 0.0)),
            ScalarValue::Str(value) => match value.to_ascii_lowercase().as_str() {
                "true" | "t" | "1" => Ok(ScalarValue::Bool(true)),
                "false" | "f" | "0" => Ok(ScalarValue::Bool(false)),
                _ => Err(QueryError::General(
                    "cannot cast value to BOOLEAN".to_string(),
                )),
            },
            ScalarValue::Null => Ok(ScalarValue::Null),
        },
        DataType::Text => Ok(ScalarValue::Str(match value {
            ScalarValue::Bool(value) => value.to_string(),
            ScalarValue::Int(value) => value.to_string(),
            ScalarValue::Float(value) => value.to_string(),
            ScalarValue::Str(value) => value.clone(),
            ScalarValue::Null => String::new(),
        })),
        DataType::Char { length } => {
            let value = cast_to_text(value)
                .filter(|value| value.chars().count() <= length.unwrap_or(1) as usize)
                .ok_or_else(|| QueryError::General("cannot cast value to CHAR".to_string()))?;
            Ok(ScalarValue::Str(value))
        }
        DataType::Varchar { length } => {
            let value = cast_to_text(value)
                .filter(|value| {
                    length.is_none_or(|length| value.chars().count() <= length as usize)
                })
                .ok_or_else(|| QueryError::General("cannot cast value to VARCHAR".to_string()))?;
            Ok(ScalarValue::Str(value))
        }
        DataType::Bytea => {
            let value = value
                .as_str()
                .ok_or_else(|| QueryError::General("cannot cast value to BYTEA".to_string()))?;
            decode_bytea(value)?;
            Ok(ScalarValue::Str(value.to_string()))
        }
        DataType::Uuid => {
            let value = value
                .as_str()
                .ok_or_else(|| QueryError::General("cannot cast value to UUID".to_string()))?;
            Uuid::parse_str(value)
                .map_err(|_| QueryError::General("cannot cast value to UUID".to_string()))?;
            Ok(ScalarValue::Str(value.to_string()))
        }
        DataType::Date | DataType::Time | DataType::Timestamp => match value {
            ScalarValue::Str(value) => Ok(ScalarValue::Str(value.clone())),
            _ => Err(QueryError::General(
                "cannot cast value to timestamp/time/date type".to_string(),
            )),
        },
        DataType::Json => Ok(ScalarValue::Str(match value {
            ScalarValue::Bool(value) => value.to_string(),
            ScalarValue::Int(value) => value.to_string(),
            ScalarValue::Float(value) => value.to_string(),
            ScalarValue::Str(value) => value.clone(),
            ScalarValue::Null => "null".to_string(),
        })),
        DataType::Array(_) => Err(QueryError::General(
            "cannot cast scalar value to ARRAY".to_string(),
        )),
        DataType::Vector(_) => Err(QueryError::General(
            "cannot cast scalar value to VECTOR".to_string(),
        )),
    }
}

fn cast_to_text(value: &ScalarValue) -> Option<String> {
    match value {
        ScalarValue::Bool(value) => Some(value.to_string()),
        ScalarValue::Int(value) => Some(value.to_string()),
        ScalarValue::Float(value) => Some(value.to_string()),
        ScalarValue::Str(value) => Some(value.clone()),
        ScalarValue::Null => None,
    }
}

fn scalar_to_i64(value: &ScalarValue) -> Option<i64> {
    match value {
        ScalarValue::Int(value) => Some(*value),
        ScalarValue::Bool(value) => Some(if *value { 1 } else { 0 }),
        ScalarValue::Float(value) if value.is_finite() && value.fract() == 0.0 => {
            Some(*value as i64)
        }
        ScalarValue::Float(_) => None,
        ScalarValue::Str(value) => value.parse().ok(),
        ScalarValue::Null => None,
    }
}

fn decode_bytea(value: &str) -> Result<(), QueryError> {
    if !value.starts_with("\\x") {
        return Err(QueryError::General(
            "cannot cast value to BYTEA".to_string(),
        ));
    }
    if (value.len() - 2).rem_euclid(2) != 0 {
        return Err(QueryError::General(
            "cannot cast value to BYTEA".to_string(),
        ));
    }
    for byte in value.as_bytes()[2..].iter() {
        if !byte.is_ascii_hexdigit() {
            return Err(QueryError::General(
                "cannot cast value to BYTEA".to_string(),
            ));
        }
    }
    Ok(())
}

fn binary_scalar(left: &ScalarValue, op: &BinaryOp, right: &ScalarValue) -> ScalarValue {
    let op = op.clone();
    match op {
        BinaryOp::And => ScalarValue::Bool(left.as_bool() && right.as_bool()),
        BinaryOp::Or => ScalarValue::Bool(left.as_bool() || right.as_bool()),
        BinaryOp::Eq => ScalarValue::Bool(eq_value(left, right)),
        BinaryOp::NotEq => ScalarValue::Bool(!eq_value(left, right)),
        BinaryOp::Lt => ScalarValue::Bool(number_cmp(left, right, |l, r| l < r)),
        BinaryOp::Lte => ScalarValue::Bool(number_cmp(left, right, |l, r| l <= r)),
        BinaryOp::Gt => ScalarValue::Bool(number_cmp(left, right, |l, r| l > r)),
        BinaryOp::Gte => ScalarValue::Bool(number_cmp(left, right, |l, r| l >= r)),
        BinaryOp::Like => ScalarValue::Bool(like_match(left.as_str(), right.as_str())),
        BinaryOp::Add => ScalarValue::Float(binary_math(left, right, |a, b| a + b)),
        BinaryOp::Sub => ScalarValue::Float(binary_math(left, right, |a, b| a - b)),
        BinaryOp::Mul => ScalarValue::Float(binary_math(left, right, |a, b| a * b)),
        BinaryOp::Div => {
            ScalarValue::Float(binary_math(
                left,
                right,
                |a, b| {
                    if b == 0.0 {
                        0.0
                    } else {
                        a / b
                    }
                },
            ))
        }
        BinaryOp::PgvectorCosine => ScalarValue::Float(vector_distance(op, left, right)),
        BinaryOp::PgvectorL2 => ScalarValue::Float(vector_distance(op, left, right)),
        BinaryOp::PgvectorDot => ScalarValue::Float(vector_distance(op, left, right)),
    }
}

fn like_match(value: Option<&str>, pattern: Option<&str>) -> bool {
    match (value, pattern) {
        (Some(value), Some(pattern)) => {
            let value = value.to_lowercase();
            let pattern = pattern.to_lowercase();
            if pattern == "%" {
                return true;
            }
            if !pattern.starts_with('%') && !pattern.ends_with('%') {
                return value == pattern;
            }
            if pattern.starts_with('%') && pattern.ends_with('%') {
                let contains_expr = pattern.trim_matches('%');
                return value.contains(contains_expr);
            }
            if pattern.starts_with('%') {
                let suffix = pattern.trim_start_matches('%');
                return value.ends_with(suffix);
            }
            if pattern.ends_with('%') {
                let prefix = pattern.trim_end_matches('%');
                return value.starts_with(prefix);
            }
            false
        }
        _ => false,
    }
}

fn number_cmp(left: &ScalarValue, right: &ScalarValue, cmp: impl Fn(f64, f64) -> bool) -> bool {
    cmp(left.to_f64().unwrap_or(0.0), right.to_f64().unwrap_or(0.0))
}

fn binary_math(left: &ScalarValue, right: &ScalarValue, op: impl Fn(f64, f64) -> f64) -> f64 {
    op(left.to_f64().unwrap_or(0.0), right.to_f64().unwrap_or(0.0))
}

fn vector_distance(op: BinaryOp, left: &ScalarValue, right: &ScalarValue) -> f64 {
    let left = left
        .as_str()
        .and_then(parse_vector_text)
        .unwrap_or_default();
    let right = right
        .as_str()
        .and_then(parse_vector_text)
        .unwrap_or_default();
    if left.is_empty() || right.is_empty() || left.len() != right.len() {
        return f64::INFINITY;
    }

    match op {
        BinaryOp::PgvectorCosine => crate::vector::cosine_distance(&left, &right),
        BinaryOp::PgvectorL2 => crate::vector::l2_distance(&left, &right),
        BinaryOp::PgvectorDot => -crate::vector::dot_score(&left, &right),
        _ => 0.0,
    }
}

fn eq_value(left: &ScalarValue, right: &ScalarValue) -> bool {
    match (left, right) {
        (ScalarValue::Null, ScalarValue::Null) => true,
        (ScalarValue::Bool(left), ScalarValue::Bool(right)) => left == right,
        (ScalarValue::Int(left), ScalarValue::Int(right)) => left == right,
        (ScalarValue::Float(left), ScalarValue::Float(right)) => left == right,
        (ScalarValue::Str(left), ScalarValue::Str(right)) => left == right,
        (ScalarValue::Int(left), ScalarValue::Float(right)) => (*left as f64) == *right,
        (ScalarValue::Float(left), ScalarValue::Int(right)) => *left == (*right as f64),
        (ScalarValue::Bool(left), ScalarValue::Int(right)) => {
            (*left && *right != 0) || (!*left && *right == 0)
        }
        (ScalarValue::Int(left), ScalarValue::Bool(right)) => {
            (*left != 0 && *right) || (*left == 0 && !*right)
        }
        (ScalarValue::Bool(left), ScalarValue::Float(right)) => {
            (*left && *right != 0.0) || (!*left && *right == 0.0)
        }
        (ScalarValue::Float(left), ScalarValue::Bool(right)) => {
            (*left != 0.0 && *right) || (*left == 0.0 && !*right)
        }
        (ScalarValue::Null, _) | (_, ScalarValue::Null) => false,
        _ => false,
    }
}

fn scalar_from_value(value: &Value) -> Option<ScalarValue> {
    match value {
        Value::Bool(v) => Some(ScalarValue::Bool(*v)),
        Value::Int64(v) => Some(ScalarValue::Int(*v)),
        Value::Float64(v) => Some(ScalarValue::Float(*v)),
        Value::String(v) => Some(ScalarValue::Str(v.clone())),
        Value::Vector(v) => Some(ScalarValue::Str(format!(
            "[{}]",
            v.values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ))),
        Value::Json(v) => Some(ScalarValue::Str(v.to_string())),
        Value::Null => Some(ScalarValue::Null),
    }
}

fn evaluate_function<R: RowAccess + ?Sized>(
    function: &FunctionCall,
    row: &R,
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    local_args: Option<&HashMap<String, Value>>,
) -> Result<Value, QueryError> {
    let name = function.name.to_ascii_lowercase();
    if name == "coalesce" {
        return evaluate_coalesce(
            function,
            row,
            params,
            search_context,
            user_functions,
            session,
            local_args,
        );
    }

    let args: Vec<Value> = function
        .args
        .iter()
        .map(|arg| {
            evaluate_expr_value(
                row,
                arg,
                params,
                search_context,
                user_functions,
                session,
                local_args,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    let cacheable = name != "coalesce" && has_only_constant_args(&function.args);
    let cache_key = if cacheable {
        Some(function_cache_key(&name, &args))
    } else {
        None
    };

    if cacheable {
        if let Some(cached) =
            FUNCTION_CACHE.with_borrow(|fc| fc.lookup(cache_key.unwrap()).cloned())
        {
            return Ok(cached);
        }
    }

    let result = match name.as_str() {
        "version" => {
            if !args.is_empty() {
                return Err(QueryError::General(format!("{} requires 0 args", name)));
            }
            Ok(Value::String(env!("CARGO_PKG_VERSION").to_string()))
        }
        "current_schema" => {
            if !args.is_empty() {
                return Err(QueryError::General(format!("{} requires 0 args", name)));
            }
            Ok(Value::String("public".to_string()))
        }
        "current_database" => {
            if !args.is_empty() {
                return Err(QueryError::General(format!("{} requires 0 args", name)));
            }
            Ok(Value::String(
                session
                    .and_then(|session| session.database.clone())
                    .unwrap_or_default(),
            ))
        }
        "current_user" | "session_user" | "current_role" => {
            if !args.is_empty() {
                return Err(QueryError::General(format!("{} requires 0 args", name)));
            }
            Ok(Value::String(
                session
                    .map(|session| session.user.clone())
                    .unwrap_or_else(|| "postgres".to_string()),
            ))
        }
        "length" | "len" => {
            if args.len() != 1 {
                return Err(QueryError::General(format!("{} requires 1 arg", name)));
            }
            let text = text_arg(&name, &args[0])?;
            match text {
                Some(text) => Ok(Value::Int64(text.chars().count() as i64)),
                None => Ok(Value::Null),
            }
        }
        "lower" => {
            if args.len() != 1 {
                return Err(QueryError::General(format!("{} requires 1 arg", name)));
            }
            let text = text_arg(&name, &args[0])?;
            match text {
                Some(text) => Ok(Value::String(text.to_lowercase())),
                None => Ok(Value::Null),
            }
        }
        "upper" => {
            if args.len() != 1 {
                return Err(QueryError::General(format!("{} requires 1 arg", name)));
            }
            let text = text_arg(&name, &args[0])?;
            match text {
                Some(text) => Ok(Value::String(text.to_uppercase())),
                None => Ok(Value::Null),
            }
        }
        "trim" => {
            if args.len() != 1 {
                return Err(QueryError::General(format!("{} requires 1 arg", name)));
            }
            let text = text_arg(&name, &args[0])?;
            match text {
                Some(text) => Ok(Value::String(text.trim().to_string())),
                None => Ok(Value::Null),
            }
        }
        "substring" => {
            if !(2..=3).contains(&args.len()) {
                return Err(QueryError::General(format!(
                    "{} requires 2 or 3 args, got {}",
                    name,
                    args.len()
                )));
            }
            let text = text_arg(&name, &args[0])?;
            let Some(text) = text else {
                return Ok(Value::Null);
            };
            let Some(start) = integer_arg(&name, &args[1])? else {
                return Ok(Value::Null);
            };
            let length = if args.len() == 3 {
                match integer_arg(&name, &args[2])? {
                    Some(length) => Some(length),
                    None => return Ok(Value::Null),
                }
            } else {
                None
            };
            Ok(Value::String(substring_text(&text, start, length)))
        }
        "concat" => {
            if args.is_empty() {
                return Err(QueryError::General(format!(
                    "{} requires at least 1 arg",
                    name
                )));
            }
            let mut out = String::new();
            for arg in &args {
                if !matches!(arg, Value::Null) {
                    out.push_str(&to_text(arg));
                }
            }
            Ok(Value::String(out))
        }
        "abs" => {
            if args.len() != 1 {
                return Err(QueryError::General(format!("{} requires 1 arg", name)));
            }
            match &args[0] {
                Value::Null => Ok(Value::Null),
                Value::Int64(v) => Ok(Value::Int64(v.checked_abs().unwrap_or(i64::MAX))),
                Value::Float64(v) => Ok(Value::Float64(v.abs())),
                _ => Err(QueryError::General(format!(
                    "function '{}' expects a numeric input",
                    name
                ))),
            }
        }
        "search" | "search_score" => {
            if args.len() != 2 {
                return Err(QueryError::General(format!("{} requires 2 args", name)));
            }
            let source = to_text(&args[0]);
            let query = to_text(&args[1]);
            let source_field = match &function.args[0] {
                Expr::Column(field) => Some(field.as_str()),
                _ => None,
            };
            let score = if let Some(context) = search_context {
                if source_field.is_none() && context.total_documents() > 0 {
                    context.score_text(None, &source, &query)
                } else {
                    context.score_text(source_field, &source, &query)
                }
            } else {
                simple_search_score(&source, &query)
            };
            if name.eq_ignore_ascii_case("search") {
                Ok(Value::Bool(score > 0.0))
            } else {
                Ok(Value::Float64(score))
            }
        }
        "vector_distance" => {
            let (left, right) = vector_operands(function, &args)?;
            Ok(Value::Float64(crate::vector::l2_distance(&left, &right)))
        }
        "vector_score" => {
            let (left, right) = vector_operands(function, &args)?;
            Ok(Value::Float64(
                1.0 / (1.0 + crate::vector::l2_distance(&left, &right)),
            ))
        }
        "cosine_distance" => {
            let (left, right) = vector_operands(function, &args)?;
            Ok(Value::Float64(crate::vector::cosine_distance(
                &left, &right,
            )))
        }
        "dot_product" => {
            let (left, right) = vector_operands(function, &args)?;
            Ok(Value::Float64(crate::vector::dot_score(&left, &right)))
        }
        "hybrid_score" => {
            if args.len() != 2 {
                return Err(QueryError::General(
                    "hybrid_score requires 2 args".to_string(),
                ));
            }
            Ok(Value::Float64(crate::hybrid::hybrid_score(
                scalar_to_f64(&args[0]),
                scalar_to_f64(&args[1]),
                None,
            )))
        }
        "snippet" => {
            if args.len() != 2 {
                return Err(QueryError::General(format!(
                    "snippet requires 2 args, got {}",
                    args.len()
                )));
            }
            let source = to_text(&args[0]);
            let query = to_text(&args[1]);
            let terms = crate::search::tokenizer::tokenize(&query);
            Ok(Value::String(crate::search::snippet(&source, &terms)))
        }
        "cast" => {
            if args.len() != 2 {
                return Err(QueryError::General(format!(
                    "cast requires 2 args, got {}",
                    args.len()
                )));
            }
            let target = DataType::parse_sql(&to_text(&args[1]))
                .map_err(|_| QueryError::General("cannot cast value".to_string()))?;
            let scalar = scalar_from_value(&args[0])
                .ok_or_else(|| QueryError::General("invalid cast input".to_string()))?;
            let value = cast_scalar(&scalar, &target)?;
            Ok(value.to_value())
        }
        _ => {
            let Some(metadata) = user_functions.get(&name) else {
                return Err(QueryError::General(format!(
                    "unsupported function '{name}'",
                )));
            };

            if args.len() != metadata.args.len() {
                return Err(QueryError::General(format!(
                    "function '{name}' expects {} args, got {}",
                    metadata.args.len(),
                    args.len()
                )));
            }

            let body = crate::sql::parser::parse_expression(&metadata.body).map_err(|error| {
                QueryError::General(format!("invalid function body for '{}': {}", name, error.0))
            })?;

            let locals = metadata
                .args
                .iter()
                .cloned()
                .zip(args)
                .map(|(arg, value)| (arg.name.to_ascii_lowercase(), value))
                .collect::<HashMap<String, Value>>();

            let merged_args = if let Some(outer) = local_args {
                let mut merged = outer.clone();
                for (name, value) in locals {
                    merged.insert(name, value);
                }
                merged
            } else {
                locals
            };

            eval_scalar(
                row,
                &body,
                params,
                search_context,
                user_functions,
                Some(&merged_args),
                session,
            )
            .map(|value| value.to_value())
        }
    };

    if let Some(key) = cache_key {
        if let Ok(ref value) = result {
            FUNCTION_CACHE.with_borrow_mut(|fc| fc.store(key, value.clone()));
        }
    }

    result
}

fn evaluate_coalesce<R: RowAccess + ?Sized>(
    function: &FunctionCall,
    row: &R,
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    local_args: Option<&HashMap<String, Value>>,
) -> Result<Value, QueryError> {
    if function.args.is_empty() {
        return Err(QueryError::General(
            "coalesce requires at least 1 arg".to_string(),
        ));
    }

    for arg in &function.args {
        let value = evaluate_expr_value(
            row,
            arg,
            params,
            search_context,
            user_functions,
            session,
            local_args,
        )?;
        if !matches!(value, Value::Null) {
            return Ok(value);
        }
    }

    Ok(Value::Null)
}

fn text_arg(name: &str, value: &Value) -> Result<Option<String>, QueryError> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(text.clone())),
        _ => Err(QueryError::General(format!(
            "function '{}' expects text input",
            name
        ))),
    }
}

fn integer_arg(name: &str, value: &Value) -> Result<Option<usize>, QueryError> {
    match value {
        Value::Null => Ok(None),
        Value::Int64(value) if *value >= 0 => Ok(Some(*value as usize)),
        Value::Float64(value)
            if value.is_finite() && *value >= 0.0 && value.fract().abs() < f64::EPSILON =>
        {
            Ok(Some(*value as usize))
        }
        _ => Err(QueryError::General(format!(
            "function '{}' expects a non-negative integer input",
            name
        ))),
    }
}

fn substring_text(value: &str, start: usize, length: Option<usize>) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return String::new();
    }

    let start_index = start.max(1).saturating_sub(1);
    if start_index >= chars.len() {
        return String::new();
    }

    let end_index = match length {
        Some(length) => start_index.saturating_add(length).min(chars.len()),
        None => chars.len(),
    };

    chars[start_index..end_index].iter().collect()
}

fn scalar_to_f64(value: &Value) -> f64 {
    match value {
        Value::Float64(v) => *v,
        Value::Int64(v) => *v as f64,
        Value::Bool(v) => v.then_some(1.0).unwrap_or(0.0),
        Value::String(v) => v.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn to_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Json(value) => value.to_string(),
        Value::Int64(value) => value.to_string(),
        Value::Float64(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Vector(value) => value
            .values
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(","),
        Value::Null => String::new(),
    }
}

fn simple_search_score(haystack: &str, query: &str) -> f64 {
    if query.trim().is_empty() {
        return 0.0;
    }

    let haystack_tokens = crate::search::tokenizer::tokenize(haystack)
        .into_iter()
        .collect::<HashSet<_>>();
    let query_tokens = crate::search::tokenizer::tokenize(query);

    if query_tokens.is_empty() {
        return 0.0;
    }

    let mut hits = 0f64;
    for token in query_tokens.iter() {
        if haystack_tokens.contains(token.as_str()) {
            hits += 1.0;
        }
    }
    hits / (query_tokens.len() as f64)
}

fn token_counts(tokens: &[String]) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for token in tokens {
        out.entry(token.clone())
            .and_modify(|count| *count += 1)
            .or_insert(1);
    }
    out
}

pub(crate) fn prepare_query_terms(query: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut terms = Vec::new();
    for token in crate::search::tokenizer::tokenize(query) {
        if seen.insert(token.clone()) {
            terms.push(token);
        }
    }
    terms
}

fn to_vector(value: &Value) -> Option<Vec<f32>> {
    match value {
        Value::Vector(vector) => Some(vector.values.clone()),
        Value::Json(json) => json
            .as_array()
            .map(|items| {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(item.as_f64()? as f32);
                }
                Some(out)
            })
            .unwrap_or(None),
        Value::String(value) => parse_vector_text(value),
        _ => None,
    }
}

fn vector_operands(
    function: &FunctionCall,
    args: &[Value],
) -> Result<(Vec<f32>, Vec<f32>), QueryError> {
    if args.len() != 2 {
        return Err(QueryError::General(format!(
            "{} requires 2 args",
            function.name
        )));
    }

    let left = to_vector(&args[0]).ok_or_else(|| {
        QueryError::General(format!(
            "{} expects vector in first argument",
            function.name
        ))
    })?;
    let right = to_vector(&args[1]).ok_or_else(|| {
        QueryError::General(format!(
            "{} expects vector in second argument",
            function.name
        ))
    })?;

    if left.len() != right.len() {
        return Err(QueryError::General(format!(
            "{} vector length mismatch: {} != {}",
            function.name,
            left.len(),
            right.len()
        )));
    }

    Ok((left, right))
}

fn parse_vector_text(value: &str) -> Option<Vec<f32>> {
    let trimmed = value.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return None;
    }

    let inner = trimmed.trim_start_matches('[').trim_end_matches(']');
    if inner.trim().is_empty() {
        return Some(Vec::new());
    }

    let mut out = Vec::new();
    for part in inner.split(',') {
        out.push(part.trim().parse::<f32>().ok()?);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::batch::BatchRow;

    #[test]
    fn should_score_term_stats_same_as_text_scoring() {
        // Arrange
        let row = BatchRow::new(vec![(
            "body".to_string(),
            Value::String("alpha beta alpha".to_string()),
        )]);
        let text_fields = vec!["body".to_string()];
        let search_context = SearchContext::from_rows(
            std::iter::once(&row),
            &text_fields,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        let term_stats = SearchTermStats::from_text(Some("alpha beta alpha"));
        let query_terms = prepare_query_terms("alpha beta");

        // Act
        let direct_score =
            search_context.score_text(Some("body"), "alpha beta alpha", "alpha beta");
        let stats_score = search_context.score_term_stats(Some("body"), &term_stats, &query_terms);

        // Assert
        assert!((direct_score - stats_score).abs() < f64::EPSILON);
    }

    #[test]
    fn should_build_search_context_from_term_stats_with_same_statistics_as_rows() {
        // Arrange
        let rows = [
            BatchRow::new(vec![(
                "body".to_string(),
                Value::String("alpha beta".to_string()),
            )]),
            BatchRow::new(vec![("body".to_string(), Value::String(String::new()))]),
            BatchRow::new(vec![("body".to_string(), Value::Null)]),
        ];
        let text_fields = vec!["body".to_string()];
        let row_context = SearchContext::from_rows(
            rows.iter(),
            &text_fields,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        let term_stats = [
            SearchTermStats::from_text(Some("alpha beta")),
            SearchTermStats::from_text(Some("")),
            SearchTermStats::from_text(None),
        ];

        // Act
        let stats_context = SearchContext::from_term_stats(
            "body",
            term_stats.iter(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );

        // Assert
        assert_eq!(row_context.total_documents, stats_context.total_documents);
        assert_eq!(row_context.doc_frequency, stats_context.doc_frequency);
        assert_eq!(row_context.avg_doc_length, stats_context.avg_doc_length);
    }

    #[test]
    fn should_score_single_field_term_stats_same_as_generic_context_with_custom_options() {
        // Arrange
        let documents = [
            SearchTermStats::from_text(Some("alpha beta alpha")),
            SearchTermStats::from_text(Some("alpha gamma")),
            SearchTermStats::from_text(Some("beta gamma")),
        ];
        let query_terms = prepare_query_terms("alpha beta");
        let source_stats = SearchTermStats::from_text(Some("alpha beta alpha"));
        let mut field_boost = HashMap::new();
        field_boost.insert("body".to_string(), 2.5);
        let mut field_k1 = HashMap::new();
        field_k1.insert("body".to_string(), 1.7);
        let mut field_b = HashMap::new();
        field_b.insert("body".to_string(), 0.3);
        let generic_context = SearchContext::from_term_stats(
            "body",
            documents.iter(),
            &field_boost,
            &field_k1,
            &field_b,
        );
        let single_field_context = SingleFieldSearchContext::from_term_stats(
            "body",
            documents.iter(),
            &field_boost,
            &field_k1,
            &field_b,
        );

        // Act
        let generic_score =
            generic_context.score_term_stats(Some("body"), &source_stats, &query_terms);
        let single_field_score = single_field_context.score_term_stats(&source_stats, &query_terms);

        // Assert
        assert!((generic_score - single_field_score).abs() < f64::EPSILON);
    }

    #[test]
    fn should_score_single_field_term_stats_as_zero_for_empty_or_missing_text() {
        // Arrange
        let documents = [
            SearchTermStats::from_text(Some("alpha beta")),
            SearchTermStats::from_text(Some("gamma")),
        ];
        let query_terms = prepare_query_terms("alpha");
        let context = SingleFieldSearchContext::from_term_stats(
            "body",
            documents.iter(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        let empty_stats = SearchTermStats::from_text(Some(""));
        let missing_stats = SearchTermStats::from_text(None);

        // Act
        let empty_score = context.score_term_stats(&empty_stats, &query_terms);
        let missing_score = context.score_term_stats(&missing_stats, &query_terms);

        // Assert
        assert_eq!(empty_score, 0.0);
        assert_eq!(missing_score, 0.0);
    }
}
