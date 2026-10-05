//! Declared parameter NULL, command RETURNING and stored NULL are separate owners.

use crate::support_type_codec_matrix::CodecCase;
use crate::support_type_metadata_contract as fixture;

pub fn cycles(prefix: &str, cases: &'static [CodecCase]) -> Vec<fixture::Cycle> {
    let mut cycles = Vec::new();
    for case in cases {
        let mut key = 1;
        for input_format in [0, 1] {
            for result_format in [0, 1] {
                let sql = format!(
                    "INSERT INTO {prefix}_{} (k, v) VALUES ({key}, $1) RETURNING v",
                    case.name
                );
                cycles.push(fixture::execute_cycle(
                    &sql,
                    Some((case.oid, input_format, None)),
                    result_format,
                    true,
                ));
                key += 1;
            }
        }
        for format in [0, 1] {
            let sql = format!("SELECT v FROM {prefix}_{} WHERE k = 1", case.name);
            cycles.push(fixture::execute_cycle(&sql, None, format, true));
        }
    }
    cycles
}

pub fn assert_batches(batches: &[fixture::Frames], cases: &[CodecCase]) {
    assert_eq!(batches.len(), cases.len() * 6);
    for (case, frames) in cases.iter().zip(batches.as_chunks::<6>().0) {
        for (index, frames) in frames.iter().enumerate() {
            let format = i16::try_from(index % 2).expect("result format");
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(
                frames,
                (case.oid, case.typlen, case.typmod),
                format,
                true,
            );
            assert_eq!(
                fixture::data_row_payloads(frames),
                vec![vec![0, 1, 255, 255, 255, 255]],
                "{} declared NULL format {format}",
                case.sql_type,
            );
        }
    }
}
