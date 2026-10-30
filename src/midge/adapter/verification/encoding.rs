pub(super) fn write_json_canonical(value: &serde_json::Value, out: &mut Vec<u8>) {
    match value {
        serde_json::Value::Null => out.push(b'0'),
        serde_json::Value::Bool(value) => {
            out.push(b'b');
            out.push(u8::from(*value));
        }
        serde_json::Value::Number(value) => {
            out.push(b'n');
            write_str(&value.to_string(), out);
        }
        serde_json::Value::String(value) => {
            out.push(b's');
            write_str(value, out);
        }
        serde_json::Value::Array(values) => {
            out.push(b'a');
            write_u64(values.len() as u64, out);
            for value in values {
                write_json_canonical(value, out);
            }
        }
        serde_json::Value::Object(values) => {
            out.push(b'o');
            write_u64(values.len() as u64, out);
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_by_key(|(key, _)| *key);
            for (key, value) in entries {
                write_str(key, out);
                write_json_canonical(value, out);
            }
        }
    }
}

pub(super) fn digest_hex(input: &[u8]) -> String {
    let mut left = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d_u128;
    let mut right = 0x0000_0000_0100_0000_0000_0000_0000_013b_u128;
    for byte in input {
        left ^= u128::from(*byte);
        left = left.wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b_u128);
        right ^= u128::from(byte.rotate_left(1));
        right = right.wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_0159_u128);
    }
    format!("{:032x}", left ^ right.rotate_left(17))
}

pub(super) fn write_str(value: &str, out: &mut Vec<u8>) {
    write_u64(value.len() as u64, out);
    out.extend_from_slice(value.as_bytes());
}

pub(super) fn write_option_str(value: Option<&str>, out: &mut Vec<u8>) {
    match value {
        Some(value) => {
            out.push(1);
            write_str(value, out);
        }
        None => out.push(0),
    }
}

pub(super) fn write_u64(value: u64, out: &mut Vec<u8>) {
    out.extend_from_slice(&value.to_be_bytes());
}
