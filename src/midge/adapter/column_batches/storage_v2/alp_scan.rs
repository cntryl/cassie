use std::collections::BTreeMap;

use super::{find_field_chunk, valid_chunk_bytes};
use crate::midge::adapter::column_batch_format_v2::{
    alp_scaled_to_f64, decode_alp_scaled, scale_alp_value, Codec, LogicalType,
};
use crate::midge::adapter::column_batches::{
    CassieError, ColumnBatchChunkMeta, ColumnBatchRow, ColumnBatchScanFallbackReason,
    ColumnBatchScanFilter, ColumnBatchScanOp, ColumnBatchSegmentMeta, Midge,
    CURRENT_COLUMN_BATCH_CODEC_VERSION,
};

pub(super) struct LoadedAlpField {
    pub(super) values: Vec<Option<i64>>,
    pub(super) scale: u8,
    pub(super) encoded_len: usize,
    pub(super) decoded_len: usize,
}

pub(super) fn single_alp_predicate_field<'a>(
    segment: &'a ColumnBatchSegmentMeta,
    filter: &ColumnBatchScanFilter,
) -> Option<(&'a String, &'a ColumnBatchChunkMeta)> {
    let first = filter.predicates.first()?;
    if !filter
        .predicates
        .iter()
        .all(|predicate| predicate.field.eq_ignore_ascii_case(&first.field))
    {
        return None;
    }
    find_field_chunk(segment, &first.field).filter(|(_, meta)| {
        meta.codec_id == Codec::Alp as u8
            && meta.codec_name == Codec::Alp.name()
            && meta.logical_type == LogicalType::Float64.name()
    })
}

pub(super) fn load_alp_field_chunk(
    tx: &cntryl_midge::Transaction,
    relation_id: u64,
    index_id: u64,
    segment: &ColumnBatchSegmentMeta,
    field: &str,
    meta: &ColumnBatchChunkMeta,
) -> Result<Result<LoadedAlpField, ColumnBatchScanFallbackReason>, CassieError> {
    if meta.codec_version != CURRENT_COLUMN_BATCH_CODEC_VERSION {
        return Ok(Err(ColumnBatchScanFallbackReason::SegmentCodecMismatch));
    }
    let Some(raw) = tx
        .get(&Midge::column_batch_field_key(
            relation_id,
            index_id,
            segment.segment_id,
            segment.revision,
            field,
        ))
        .map_err(CassieError::from)?
    else {
        return Ok(Err(ColumnBatchScanFallbackReason::SegmentMissing));
    };
    if !valid_chunk_bytes(&raw, meta) {
        return Ok(Err(ColumnBatchScanFallbackReason::SegmentChecksumMismatch));
    }
    let Ok(decoded) = decode_alp_scaled(&raw) else {
        return Ok(Err(ColumnBatchScanFallbackReason::InvalidPayload));
    };
    if decoded.values.len() != segment.row_count
        || decoded.encoded_len != meta.encoded_len
        || decoded.decoded_len != meta.decoded_len
    {
        return Ok(Err(ColumnBatchScanFallbackReason::SegmentCodecMismatch));
    }
    Ok(Ok(LoadedAlpField {
        values: decoded.values,
        scale: decoded.scale,
        encoded_len: decoded.encoded_len,
        decoded_len: decoded.decoded_len,
    }))
}

pub(super) fn alp_selection_for_filter(
    values: &[Option<i64>],
    scale: u8,
    filter: &ColumnBatchScanFilter,
) -> Option<(Vec<bool>, usize)> {
    let mut selection = vec![true; values.len()];
    let mut predicate_values = 0usize;
    for predicate in &filter.predicates {
        let expected = match predicate.op {
            ColumnBatchScanOp::IsNull | ColumnBatchScanOp::IsNotNull => None,
            ColumnBatchScanOp::Eq
            | ColumnBatchScanOp::Lt
            | ColumnBatchScanOp::Lte
            | ColumnBatchScanOp::Gt
            | ColumnBatchScanOp::Gte => Some(scale_alp_value(predicate.value.as_ref()?, scale)?),
        };
        for (selected, value) in selection.iter_mut().zip(values) {
            if *selected {
                *selected = alp_predicate_matches(*value, predicate.op, expected);
            }
            predicate_values = predicate_values.saturating_add(1);
        }
    }
    Some((selection, predicate_values))
}

fn alp_predicate_matches(value: Option<i64>, op: ColumnBatchScanOp, expected: Option<i64>) -> bool {
    match op {
        ColumnBatchScanOp::IsNull => value.is_none(),
        ColumnBatchScanOp::IsNotNull => value.is_some(),
        ColumnBatchScanOp::Eq
        | ColumnBatchScanOp::Lt
        | ColumnBatchScanOp::Lte
        | ColumnBatchScanOp::Gt
        | ColumnBatchScanOp::Gte => {
            let (Some(value), Some(expected)) = (value, expected) else {
                return false;
            };
            let ordering = value.cmp(&expected);
            match op {
                ColumnBatchScanOp::Eq => ordering.is_eq(),
                ColumnBatchScanOp::Lt => ordering.is_lt(),
                ColumnBatchScanOp::Lte => !ordering.is_gt(),
                ColumnBatchScanOp::Gt => ordering.is_gt(),
                ColumnBatchScanOp::Gte => !ordering.is_lt(),
                ColumnBatchScanOp::IsNull | ColumnBatchScanOp::IsNotNull => false,
            }
        }
    }
}

pub(super) fn selected_alp_json_values(
    values: &[Option<i64>],
    scale: u8,
    selection: &[bool],
) -> Result<Vec<serde_json::Value>, ColumnBatchScanFallbackReason> {
    values
        .iter()
        .zip(selection)
        .map(|(value, selected)| {
            if !selected {
                return Ok(serde_json::Value::Null);
            }
            value.map_or(Ok(serde_json::Value::Null), |value| {
                serde_json::Number::from_f64(alp_scaled_to_f64(value, scale))
                    .map(serde_json::Value::Number)
                    .ok_or(ColumnBatchScanFallbackReason::InvalidPayload)
            })
        })
        .collect()
}

pub(super) fn materialize_selected_alp_rows(
    row_ids: Vec<String>,
    selection: Vec<bool>,
    field: &str,
    values: &[Option<i64>],
    scale: u8,
) -> Result<Vec<ColumnBatchRow>, ColumnBatchScanFallbackReason> {
    row_ids
        .into_iter()
        .zip(selection)
        .zip(values)
        .filter_map(|((row_id, selected), value)| {
            selected.then(|| {
                let value = value.map_or(Ok(serde_json::Value::Null), |value| {
                    serde_json::Number::from_f64(alp_scaled_to_f64(value, scale))
                        .map(serde_json::Value::Number)
                        .ok_or(ColumnBatchScanFallbackReason::InvalidPayload)
                })?;
                Ok(ColumnBatchRow {
                    row_id,
                    values: BTreeMap::from([(field.to_string(), value)]),
                })
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midge::adapter::ColumnBatchScanPredicate;

    fn selection(op: ColumnBatchScanOp, value: Option<serde_json::Value>) -> Vec<bool> {
        let filter = ColumnBatchScanFilter {
            predicates: vec![ColumnBatchScanPredicate {
                field: "score".to_string(),
                op,
                value,
            }],
        };
        alp_selection_for_filter(&[None, Some(-100), Some(0), Some(100)], 2, &filter)
            .expect("literal should be exactly representable")
            .0
    }

    #[test]
    fn should_match_every_supported_alp_predicate_without_float_materialization() {
        // Arrange
        let expected = [
            (ColumnBatchScanOp::Eq, vec![false, false, true, false]),
            (ColumnBatchScanOp::Lt, vec![false, true, false, false]),
            (ColumnBatchScanOp::Lte, vec![false, true, true, false]),
            (ColumnBatchScanOp::Gt, vec![false, false, false, true]),
            (ColumnBatchScanOp::Gte, vec![false, false, true, true]),
        ];

        // Act
        let actual = expected
            .iter()
            .map(|(op, _)| selection(*op, Some(serde_json::json!(0.0))))
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(
            actual,
            expected
                .into_iter()
                .map(|(_, expected)| expected)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            selection(ColumnBatchScanOp::IsNull, None),
            vec![true, false, false, false]
        );
        assert_eq!(
            selection(ColumnBatchScanOp::IsNotNull, None),
            vec![false, true, true, true]
        );
    }

    #[test]
    fn should_defer_non_exact_alp_literals_to_general_float_semantics() {
        // Arrange
        let filter = ColumnBatchScanFilter {
            predicates: vec![ColumnBatchScanPredicate {
                field: "score".to_string(),
                op: ColumnBatchScanOp::Gt,
                value: Some(serde_json::json!(0.333)),
            }],
        };

        // Act
        let selection = alp_selection_for_filter(&[Some(33), Some(34)], 2, &filter);

        // Assert
        assert!(selection.is_none());
    }

    #[test]
    fn should_materialize_only_selected_alp_positions_without_losing_nulls() {
        // Arrange
        let values = [Some(-125), None, Some(250)];
        let selection = [true, true, false];

        // Act
        let projected =
            selected_alp_json_values(&values, 2, &selection).expect("project selected ALP values");
        let rows = materialize_selected_alp_rows(
            vec![
                "row-0".to_string(),
                "row-1".to_string(),
                "row-2".to_string(),
            ],
            selection.to_vec(),
            "score",
            &values,
            2,
        )
        .expect("materialize selected ALP rows");

        // Assert
        assert_eq!(
            projected,
            vec![
                serde_json::json!(-1.25),
                serde_json::Value::Null,
                serde_json::Value::Null
            ]
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].row_id, "row-0");
        assert_eq!(rows[0].values["score"], serde_json::json!(-1.25));
        assert_eq!(rows[1].row_id, "row-1");
        assert!(rows[1].values["score"].is_null());
    }
}
