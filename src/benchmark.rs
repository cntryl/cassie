//! Narrow access to production kernels used by Cassie's benchmark executables.

use std::collections::HashMap;
use std::time::Instant;

use serde_json::json;

use crate::catalog::FunctionMeta;
use crate::config::CassieRuntimeLimits;
use crate::executor::batch::{flatten_batches, BatchRow};
use crate::executor::sort::EvalInput;
use crate::executor::{filter, projection, sort};
use crate::midge::row_blob::{decode_row, encode_row, RowSchema};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{BinaryOp, Expr, OrderExpr, SelectItem, SortDirection};
use crate::types::{DataType, FieldSchema, Schema, Value};

/// Observed output from one benchmark kernel or subsystem operation.
pub struct KernelObservation {
    completed_operations: u64,
    result_cardinality: u64,
    candidate_count: Option<u64>,
    peak_query_memory_bytes: Option<u64>,
    after_sample: Option<Box<dyn FnOnce()>>,
}

impl KernelObservation {
    #[must_use]
    pub const fn new(completed_operations: u64, result_cardinality: u64) -> Self {
        Self {
            completed_operations,
            result_cardinality,
            candidate_count: None,
            peak_query_memory_bytes: None,
            after_sample: None,
        }
    }

    #[must_use]
    pub const fn with_candidate_count(mut self, candidate_count: u64) -> Self {
        self.candidate_count = Some(candidate_count);
        self
    }

    #[must_use]
    pub const fn with_peak_query_memory_bytes(mut self, peak: u64) -> Self {
        self.peak_query_memory_bytes = Some(peak);
        self
    }

    #[must_use]
    pub fn with_after_sample(mut self, after_sample: impl FnOnce() + 'static) -> Self {
        self.after_sample = Some(Box::new(after_sample));
        self
    }

    #[must_use]
    pub const fn completed_operations(&self) -> u64 {
        self.completed_operations
    }

    #[must_use]
    pub const fn result_cardinality(&self) -> u64 {
        self.result_cardinality
    }

    #[must_use]
    pub const fn candidate_count(&self) -> Option<u64> {
        self.candidate_count
    }

    #[must_use]
    pub const fn peak_query_memory_bytes(&self) -> Option<u64> {
        self.peak_query_memory_bytes
    }

    pub fn finish_sample(mut self) {
        if let Some(after_sample) = self.after_sample.take() {
            after_sample();
        }
    }
}

/// Production binary pgwire data-row encoder input.
pub struct PgwireRowCodecKernel {
    row: Vec<Value>,
    columns: Vec<crate::executor::ColumnMeta>,
}

impl PgwireRowCodecKernel {
    #[must_use]
    pub fn sample(index: usize) -> Self {
        Self {
            row: vec![
                Value::String(format!("doc-{index}")),
                Value::Int64(i64::try_from(index).expect("pgwire fixture index should fit i64")),
            ],
            columns: vec![
                crate::executor::ColumnMeta::text("id"),
                crate::executor::ColumnMeta::from_data_type("score", &DataType::Int),
            ],
        }
    }

    /// Encodes one row through the frame writer used by live pgwire connections.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        crate::pgwire::connection::benchmark_encode_data_row(self.row.clone(), &self.columns, &[])
            .expect("benchmark pgwire row should encode")
    }
}

/// Production binary pgwire frontend decoder inputs.
pub struct PgwireFrontendCodecKernel {
    frames: Vec<(u8, Vec<u8>)>,
}

impl PgwireFrontendCodecKernel {
    #[must_use]
    pub fn with_frames(frames: usize) -> Self {
        let frames = (0..frames)
            .map(|index| frontend_fixture(index % 5))
            .collect();
        Self { frames }
    }

    /// Decodes every fixture with the parser used by live pgwire connections.
    #[must_use]
    pub fn decode(&self) -> usize {
        self.frames
            .iter()
            .map(|(tag, payload)| {
                crate::pgwire::connection::benchmark_decode_frontend(*tag, payload.clone())
                    .expect("benchmark pgwire frontend frame should decode")
            })
            .sum()
    }

    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
}

/// Production pgwire bind-parameter decoder inputs.
pub struct PgwireParameterBindingKernel {
    parameters: Vec<(Vec<u8>, i16, i32)>,
}

impl PgwireParameterBindingKernel {
    #[must_use]
    pub fn with_parameters(parameters: usize) -> Self {
        let fixtures: [(&[u8], i16, i32); 4] = [
            (b"42", 0, 23),
            (b"alpha", 0, 25),
            (b"true", 0, 16),
            (b"3.5", 0, 701),
        ];
        let parameters = (0..parameters)
            .map(|index| {
                let (value, format, oid) = fixtures[index % fixtures.len()];
                (value.to_vec(), format, oid)
            })
            .collect();
        Self { parameters }
    }

    /// Decodes every value through the production extended-query bind path.
    #[must_use]
    pub fn decode(&self) -> Vec<Value> {
        self.parameters
            .iter()
            .map(|(parameter, format, oid)| {
                crate::pgwire::connection::benchmark_decode_parameter(parameter, *format, *oid)
                    .expect("benchmark bind parameter should decode")
            })
            .collect()
    }

    #[must_use]
    pub fn parameter_count(&self) -> usize {
        self.parameters.len()
    }
}

fn frontend_fixture(index: usize) -> (u8, Vec<u8>) {
    match index {
        0 => {
            let mut payload = cstrings(&["bench_stmt", "SELECT $1::INT AS value"]);
            payload.extend_from_slice(&1_i16.to_be_bytes());
            payload.extend_from_slice(&23_i32.to_be_bytes());
            (b'P', payload)
        }
        1 => {
            let mut payload = cstrings(&["bench_portal", "bench_stmt"]);
            payload.extend_from_slice(&0_i16.to_be_bytes());
            payload.extend_from_slice(&1_i16.to_be_bytes());
            payload.extend_from_slice(&1_i32.to_be_bytes());
            payload.push(b'7');
            payload.extend_from_slice(&0_i16.to_be_bytes());
            (b'B', payload)
        }
        2 => {
            let mut payload = vec![b'S'];
            payload.extend_from_slice(b"bench_stmt\0");
            (b'D', payload)
        }
        3 => {
            let mut payload = cstrings(&["bench_portal"]);
            payload.extend_from_slice(&20_i32.to_be_bytes());
            (b'E', payload)
        }
        _ => (b'S', Vec::new()),
    }
}

fn cstrings(values: &[&str]) -> Vec<u8> {
    let mut payload = Vec::new();
    for value in values {
        payload.extend_from_slice(value.as_bytes());
        payload.push(0);
    }
    payload
}

/// Production `cassie-midge-layout-v1` row-codec fixture.
pub struct RowCodecKernel {
    schema: RowSchema,
    row: serde_json::Value,
}

impl RowCodecKernel {
    /// Builds the fixed row used by the Tier 1 row-codec benchmark.
    #[must_use]
    pub fn sample() -> Self {
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "id".to_string(),
                    data_type: DataType::Text,
                    nullable: false,
                },
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
                    nullable: true,
                },
            ],
        };
        Self {
            schema: RowSchema::from_schema(&schema),
            row: json!({"id": "doc-1", "title": "alpha", "score": 42}),
        }
    }

    /// Encodes the fixture through Cassie's production binary row codec.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        encode_row(&self.schema, &self.row).expect("benchmark row fixture should encode")
    }

    /// Decodes bytes through Cassie's production binary row codec.
    #[must_use]
    pub fn decode(&self, encoded: &[u8]) -> serde_json::Value {
        decode_row(&self.schema, encoded).expect("benchmark row fixture should decode")
    }

    /// Encodes and decodes the fixed fixture through the production codec.
    #[must_use]
    pub fn round_trip(&self) -> (Vec<u8>, serde_json::Value) {
        let encoded = self.encode();
        let decoded = self.decode(&encoded);
        (encoded, decoded)
    }

    /// Returns the semantic row expected after decoding.
    #[must_use]
    pub fn expected_row(&self) -> &serde_json::Value {
        &self.row
    }
}

impl Default for RowCodecKernel {
    fn default() -> Self {
        Self::sample()
    }
}

/// Production `cassie-midge-layout-v1` row-key fixture.
pub struct RowKeyKernel {
    relation_id: u64,
    id: String,
}

impl RowKeyKernel {
    /// Builds a fixture for one persisted relation and document identity.
    #[must_use]
    pub fn for_row(relation_id: u64, id: impl Into<String>) -> Self {
        Self {
            relation_id,
            id: id.into(),
        }
    }

    /// Encodes the key and decodes its row-id component with production layout helpers.
    #[must_use]
    pub fn encode_decode(&self) -> (Vec<u8>, String) {
        crate::midge::adapter::benchmark_row_key_round_trip(self.relation_id, &self.id)
            .expect("benchmark row key should round trip")
    }
}

impl Default for RowKeyKernel {
    fn default() -> Self {
        Self::for_row(7, "doc-1")
    }
}

/// Fixed inputs for Cassie's production predicate, filter, projection, and top-k kernels.
pub struct ExecutorKernel {
    predicate_row: BatchRow,
    predicate: Expr,
    filter_rows: Vec<BatchRow>,
    filter_predicate: Expr,
    filter_params: Vec<Value>,
    projection_rows: Vec<BatchRow>,
    projection: Vec<SelectItem>,
    comparison_rows: Vec<BatchRow>,
    comparison: Expr,
    top_k_rows: Vec<BatchRow>,
    top_k_order: Vec<OrderExpr>,
    user_functions: HashMap<String, FunctionMeta>,
}

impl ExecutorKernel {
    /// Builds all fixed executor inputs outside the measured kernel calls.
    #[must_use]
    pub fn sample() -> Self {
        let predicate = binary(
            binary(
                Expr::Column("score".to_string()),
                BinaryOp::Gte,
                Expr::NumberLiteral(40.0),
            ),
            BinaryOp::And,
            binary(
                Expr::Column("status".to_string()),
                BinaryOp::Eq,
                Expr::StringLiteral("approved".to_string()),
            ),
        );
        let filter_rows = [
            1_i64, 10, 100, 3, 25, 8, 99, 7, 44, 61, 2, 88, 13, 55, 34, 21, 5, 89, 144, 233, 377,
            610, 987, 1_597, 4, 6, 9, 12, 18, 27, 81, 243,
        ]
        .into_iter()
        .map(|score| BatchRow::new(vec![("score".to_string(), Value::Int64(score))]))
        .collect();
        let comparison_rows = [
            (1_i64, 2_i64),
            (3, 5),
            (8, 13),
            (21, 34),
            (55, 89),
            (144, 233),
            (377, 610),
            (987, 1_597),
        ]
        .into_iter()
        .map(|(left, right)| {
            BatchRow::new(vec![
                ("left".to_string(), Value::Int64(left)),
                ("right".to_string(), Value::Int64(right)),
            ])
        })
        .collect();
        let top_k_rows = [3_i64, 1, 7, 2, 9, 4]
            .into_iter()
            .map(|score| BatchRow::new(vec![("score".to_string(), Value::Int64(score))]))
            .collect();
        Self {
            predicate_row: BatchRow::new(vec![
                ("score".to_string(), Value::Int64(42)),
                ("status".to_string(), Value::String("approved".to_string())),
            ]),
            predicate,
            filter_rows,
            filter_predicate: binary(
                Expr::Column("score".to_string()),
                BinaryOp::Gte,
                Expr::Param(0),
            ),
            filter_params: vec![Value::Int64(10)],
            projection_rows: vec![BatchRow::new(vec![
                ("id".to_string(), Value::String("doc-1".to_string())),
                ("title".to_string(), Value::String("alpha".to_string())),
                ("body".to_string(), Value::String("beta".to_string())),
            ])],
            projection: vec![SelectItem::Column {
                name: "title".to_string(),
                alias: None,
            }],
            comparison_rows,
            comparison: binary(
                Expr::Column("left".to_string()),
                BinaryOp::Lt,
                Expr::Column("right".to_string()),
            ),
            top_k_rows,
            top_k_order: vec![OrderExpr {
                expr: Expr::Column("score".to_string()),
                direction: SortDirection::Desc,
                nulls: None,
            }],
            user_functions: HashMap::new(),
        }
    }

    /// Evaluates one expression with the production scalar-expression evaluator.
    #[must_use]
    pub fn predicate_matches(&self) -> bool {
        matches!(
            filter::evaluate_expr_value(
                &self.predicate_row,
                &self.predicate,
                &[],
                None,
                &self.user_functions,
                None,
                None,
            )
            .expect("benchmark predicate should evaluate"),
            Value::Bool(true)
        )
    }

    /// Filters the fixed batch with the production executor filter.
    #[must_use]
    pub fn filter_batch(&self) -> usize {
        filter::filter_rows(
            self.filter_rows.clone(),
            &self.filter_predicate,
            &self.filter_params,
            None,
            &self.user_functions,
            None,
        )
        .expect("benchmark batch should filter")
        .len()
    }

    /// Projects the fixed row with the production executor projection.
    #[must_use]
    pub fn project_row(&self) -> Vec<Value> {
        projection::project_rows(
            self.projection_rows.clone(),
            &self.projection,
            &[],
            None,
            &self.user_functions,
            None,
        )
        .expect("benchmark row should project")
        .into_iter()
        .next()
        .map_or_else(Vec::new, BatchRow::into_values)
    }

    /// Compares all fixed value pairs with the production expression evaluator.
    #[must_use]
    pub fn matching_value_comparisons(&self) -> usize {
        self.comparison_rows
            .iter()
            .filter(|row| {
                matches!(
                    filter::evaluate_expr_value(
                        *row,
                        &self.comparison,
                        &[],
                        None,
                        &self.user_functions,
                        None,
                        None,
                    )
                    .expect("benchmark comparison should evaluate"),
                    Value::Bool(true)
                )
            })
            .count()
    }

    /// Compares one fixed value pair with the production expression evaluator.
    #[must_use]
    pub fn value_comparison_matches(&self) -> bool {
        matches!(
            filter::evaluate_expr_value(
                self.comparison_rows
                    .first()
                    .expect("benchmark comparison fixture should contain a row"),
                &self.comparison,
                &[],
                None,
                &self.user_functions,
                None,
                None,
            )
            .expect("benchmark comparison should evaluate"),
            Value::Bool(true)
        )
    }

    /// Maintains and ranks the fixed top-k set with the production executor kernel.
    #[must_use]
    pub fn top_k_scores(&self) -> Vec<i64> {
        let projection = Vec::new();
        let params = Vec::new();
        let rows = sort::maintain_top_k_kernel(
            self.top_k_rows.clone(),
            &EvalInput {
                order: &self.top_k_order,
                projection: &projection,
                params: &params,
                search_context: None,
                user_functions: &self.user_functions,
                session: None,
            },
            3,
        )
        .expect("benchmark top-k kernel should evaluate");
        rows.iter()
            .filter_map(|row| row.get("score").and_then(Value::as_i64))
            .collect()
    }

    #[must_use]
    pub fn top_k_candidate_count(&self) -> usize {
        self.top_k_rows.len()
    }
}

impl Default for ExecutorKernel {
    fn default() -> Self {
        Self::sample()
    }
}

/// Fixed, bounded inputs for Tier 2 physical-operator benchmarks.
pub struct SubsystemExecutorKernel {
    rows: usize,
    fixture_rows: Vec<BatchRow>,
    filter_predicate: Expr,
    projection: Vec<SelectItem>,
    top_k_order: Vec<OrderExpr>,
    controls: QueryExecutionControls,
    user_functions: HashMap<String, FunctionMeta>,
}

impl SubsystemExecutorKernel {
    /// Builds one bounded fixture outside the measured operator calls.
    #[must_use]
    pub fn with_rows(rows: usize) -> Self {
        assert!(rows > 0, "executor benchmark fixture must not be empty");
        assert!(rows <= 2_048, "Tier 2 executor fixture exceeds 2,048 rows");

        let make_row = |index: usize| {
            let score = i64::try_from(index % 100).expect("benchmark score should fit i64");
            BatchRow::new(vec![
                ("id".to_string(), Value::String(format!("doc-{index}"))),
                (
                    "title".to_string(),
                    Value::String(format!("title-{}", index % 16)),
                ),
                ("score".to_string(), Value::Int64(score)),
            ])
        };
        let fixture_rows = (0..rows).map(make_row).collect::<Vec<_>>();
        let limits = CassieRuntimeLimits {
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        };

        Self {
            rows,
            fixture_rows,
            filter_predicate: binary(
                Expr::Column("score".to_string()),
                BinaryOp::Gte,
                Expr::Param(0),
            ),
            projection: vec![
                SelectItem::Column {
                    name: "id".to_string(),
                    alias: None,
                },
                SelectItem::Column {
                    name: "title".to_string(),
                    alias: None,
                },
            ],
            top_k_order: vec![OrderExpr {
                expr: Expr::Column("score".to_string()),
                direction: SortDirection::Desc,
                nulls: None,
            }],
            controls: QueryExecutionControls::from_limits(&limits, Instant::now()),
            user_functions: HashMap::new(),
        }
    }

    /// Runs the production physical filter over the entire fixture.
    #[must_use]
    pub fn filter(&self) -> KernelObservation {
        let output = filter::filter_rows(
            self.fixture_rows.clone(),
            &self.filter_predicate,
            &[Value::Int64(50)],
            None,
            &self.user_functions,
            None,
        )
        .expect("benchmark filter should execute");
        let output_rows = output.len();
        assert!(output_rows > 0, "benchmark filter should match rows");
        std::hint::black_box(output);
        KernelObservation::new(usize_to_u64(self.rows), usize_to_u64(output_rows))
            .with_candidate_count(usize_to_u64(self.rows))
    }

    /// Runs the production physical projection over the entire fixture.
    #[must_use]
    pub fn project(&self) -> KernelObservation {
        let output = projection::project_rows(
            self.fixture_rows.clone(),
            &self.projection,
            &[],
            None,
            &self.user_functions,
            None,
        )
        .expect("benchmark projection should execute");
        assert_eq!(output.len(), self.rows);
        std::hint::black_box(output);
        KernelObservation::new(usize_to_u64(self.rows), usize_to_u64(self.rows))
            .with_candidate_count(usize_to_u64(self.rows))
    }

    /// Runs the production physical top-k operator over the entire fixture.
    #[must_use]
    pub fn top_k(&self) -> KernelObservation {
        let projection = Vec::new();
        let params = Vec::new();
        let output = sort::top_k_batches_with_controls(
            vec![self.fixture_rows.clone()],
            &EvalInput {
                order: &self.top_k_order,
                projection: &projection,
                params: &params,
                search_context: None,
                user_functions: &self.user_functions,
                session: None,
            },
            20,
            &self.controls,
        )
        .expect("benchmark top-k should execute");
        let output_rows = flatten_batches(output.clone()).len();
        assert_eq!(output_rows, 20);
        std::hint::black_box(output);
        KernelObservation::new(usize_to_u64(self.rows), usize_to_u64(output_rows))
            .with_candidate_count(usize_to_u64(self.rows))
            .with_peak_query_memory_bytes(usize_to_u64(self.controls.peak_query_memory_bytes()))
    }
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).expect("benchmark value should fit u64")
}

fn binary(left: Expr, op: BinaryOp, right: Expr) -> Expr {
    Expr::Binary {
        left: Box::new(left),
        op,
        right: Box::new(right),
    }
}
