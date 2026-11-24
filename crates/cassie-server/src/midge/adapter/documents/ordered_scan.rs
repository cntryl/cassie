#[path = "ordered_scan/controlled.rs"]
mod controlled;

use super::super::{
    decode_projected_row, decode_projected_row_with_aliases, decode_row, CassieError, DocumentRef,
    Midge, OrderedRowBound, Query, RowDecode,
};

pub(crate) struct OrderedRowScanRequest<'a> {
    pub collection: &'a str,
    pub decode: RowDecode,
    pub start_bound: Option<&'a OrderedRowBound>,
    pub end_bound: Option<&'a OrderedRowBound>,
    pub reverse: bool,
    pub limit: Option<usize>,
}

impl Midge {
    fn ordered_start_key(prefix: &[u8], id: &str, inclusive: bool) -> Vec<u8> {
        let mut key = prefix.to_vec();
        key.extend_from_slice(id.as_bytes());
        if !inclusive {
            key.push(0);
        }
        key
    }

    fn ordered_end_key(prefix: &[u8], id: &str, inclusive: bool) -> Vec<u8> {
        let mut key = prefix.to_vec();
        key.extend_from_slice(id.as_bytes());
        if inclusive {
            key.push(0);
        }
        key
    }
}
