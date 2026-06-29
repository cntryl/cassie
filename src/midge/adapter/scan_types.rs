use std::collections::HashSet;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct DocumentRef {
    pub id: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowDecode {
    Full,
    Projected(Vec<String>),
    ProjectedHistorical(Vec<String>),
}

impl RowDecode {
    pub(crate) fn into_projection(self) -> (Option<HashSet<String>>, bool) {
        match self {
            RowDecode::Full => (None, false),
            RowDecode::Projected(fields) => (Some(normalized_projection(fields)), false),
            RowDecode::ProjectedHistorical(fields) => (Some(normalized_projection(fields)), true),
        }
    }
}

fn normalized_projection(fields: Vec<String>) -> HashSet<String> {
    fields
        .into_iter()
        .map(|field| field.to_ascii_lowercase())
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct RowFilter {
    pub field: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnBatchScanFilter {
    pub predicates: Vec<ColumnBatchScanPredicate>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnBatchScanPredicate {
    pub field: String,
    pub op: ColumnBatchScanOp,
    pub value: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnBatchScanOp {
    Eq,
    Lt,
    Lte,
    Gt,
    Gte,
    IsNull,
    IsNotNull,
}

#[derive(Debug, Clone, Copy)]
pub enum ColumnBatchScanFallbackReason {
    NoCoveringIndex,
    MissingMetadata,
    SegmentSizeMismatch,
    FieldCoverageMismatch,
    SegmentMissing,
    SegmentChecksumMismatch,
    InvalidPayload,
    InvalidEncodingVersion,
    SegmentCodecMismatch,
    SegmentDecodeFailed,
    RowFilterMismatch,
}

impl ColumnBatchScanFallbackReason {
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::NoCoveringIndex => "no_covering_column_index",
            Self::MissingMetadata => "missing_metadata",
            Self::SegmentSizeMismatch => "segment_size_mismatch",
            Self::FieldCoverageMismatch => "field_coverage_mismatch",
            Self::SegmentMissing => "segment_missing",
            Self::SegmentChecksumMismatch => "segment_checksum_mismatch",
            Self::InvalidPayload => "invalid_payload",
            Self::InvalidEncodingVersion => "invalid_encoding_version",
            Self::SegmentCodecMismatch => "segment_codec_mismatch",
            Self::SegmentDecodeFailed => "segment_decode_failed",
            Self::RowFilterMismatch => "row_filter_mismatch",
        }
    }

    #[must_use]
    pub const fn is_decode_fallback(&self) -> bool {
        matches!(
            self,
            Self::SegmentMissing
                | Self::SegmentChecksumMismatch
                | Self::InvalidPayload
                | Self::InvalidEncodingVersion
                | Self::SegmentCodecMismatch
                | Self::SegmentDecodeFailed
        )
    }
}

#[derive(Debug, Clone)]
pub enum ColumnBatchScanDecision {
    Hit(ColumnBatchScanOutcome),
    Fallback(ColumnBatchScanFallbackReason),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MidgeScanTimings {
    pub scan: Duration,
    pub row_decode: Duration,
}

#[derive(Debug, Clone)]
pub(crate) struct OrderedRowBound {
    pub id: String,
    pub inclusive: bool,
}

#[derive(Debug, Clone)]
pub struct ColumnBatchScanOutcome {
    pub batches: Vec<Vec<DocumentRef>>,
    pub timings: MidgeScanTimings,
    pub index_name: String,
    pub compressed_bytes: usize,
    pub uncompressed_bytes: usize,
    pub skipped_segments: usize,
    pub decoded_columns: usize,
}
