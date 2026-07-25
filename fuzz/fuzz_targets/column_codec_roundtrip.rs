#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    let logical_type = match data[0] % 5 {
        0 => "bigint",
        1 => "float",
        2 => "boolean",
        3 => "text",
        _ => "json",
    };
    let values = data[1..]
        .chunks(8)
        .take(4_096)
        .map(|bytes| value_for(logical_type, bytes))
        .collect::<Vec<_>>();
    let encoded =
        cassie::midge::adapter::encode_column_chunk_for_test(logical_type, &values)
            .expect("generated scalar fixture should encode");
    let decoded = cassie::midge::adapter::decode_column_chunk_for_test(&encoded)
        .expect("encoded scalar fixture should decode");
    assert_eq!(decoded, values);
});

fn value_for(logical_type: &str, bytes: &[u8]) -> serde_json::Value {
    if bytes.first().is_some_and(|byte| byte % 11 == 0) {
        return serde_json::Value::Null;
    }
    let mut padded = [0_u8; 8];
    padded[..bytes.len()].copy_from_slice(bytes);
    match logical_type {
        "bigint" => serde_json::json!(i64::from_le_bytes(padded)),
        "float" => {
            let value = f64::from_bits(u64::from_le_bytes(padded));
            serde_json::Number::from_f64(if value.is_finite() { value } else { 0.0 })
                .map_or(serde_json::Value::Null, serde_json::Value::Number)
        }
        "boolean" => serde_json::Value::Bool(padded[0] & 1 == 1),
        "text" => serde_json::Value::String(
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        ),
        _ => serde_json::json!({ "bytes": bytes }),
    }
}
