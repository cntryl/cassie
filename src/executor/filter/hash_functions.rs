//! PostgreSQL hashing and UUID builtins: `md5`, `crc32`, `crc32c`,
//! `hashtext`, `hashtextextended`, `gen_random_uuid`, `uuidv4`, `uuidv7`
//! and `uuid_extract_version`/`uuid_extract_timestamp`.
use super::{QueryError, Value};
use crate::types::typed::TypedValue;
use md5::Digest;

/// The argument marker the binder appends to `md5(bytea)`, selecting the
/// binary overload over the shared string carrier.
pub(crate) const MD5_BYTEA_MARKER: bool = true;

fn bytea(value: &Value) -> Result<Vec<u8>, QueryError> {
    let Value::String(carrier) = value else {
        return Err(QueryError::General("invalid BYTEA argument".into()));
    };
    let digits = carrier
        .strip_prefix("\\x")
        .ok_or_else(|| QueryError::General("invalid canonical BYTEA carrier".into()))?;
    (0..digits.len())
        .step_by(2)
        .map(|index| {
            digits
                .get(index..index + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| QueryError::General("invalid canonical BYTEA carrier".into()))
        })
        .collect()
}

fn text(value: &Value) -> Result<&str, QueryError> {
    match value {
        Value::String(text) => Ok(text),
        _ => Err(QueryError::General("invalid text argument".into())),
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// Evaluates a hashing or UUID builtin; `None` when `name` is not one.
pub(super) fn evaluate(name: &str, args: &[Value]) -> Option<Result<Value, QueryError>> {
    let strict = !matches!(name, "gen_random_uuid" | "uuidv4" | "uuidv7");
    if !crate::sql::functions::hashing::is_hash_function(name) {
        return None;
    }
    if strict && args.iter().any(|value| matches!(value, Value::Null)) {
        return Some(Ok(Value::Null));
    }
    Some(match (name, args) {
        ("md5", [value]) => text(value).map(|text| Value::String(hex(&md5::Md5::digest(text)))),
        ("md5", [value, Value::Bool(MD5_BYTEA_MARKER)]) => {
            bytea(value).map(|bytes| Value::String(hex(&md5::Md5::digest(bytes))))
        }
        ("crc32", [value]) => bytea(value).map(|bytes| Value::Int64(i64::from(crc32(&bytes)))),
        ("crc32c", [value]) => bytea(value).map(|bytes| Value::Int64(i64::from(crc32c(&bytes)))),
        ("hashtext", [value]) => text(value).map(|text| {
            Value::Int64(i64::from(i32::from_ne_bytes(
                hash_bytes(text.as_bytes(), 0).1.to_ne_bytes(),
            )))
        }),
        ("hashtextextended", [value, Value::Int64(seed)]) => text(value).map(|text| {
            let (high, low) = hash_bytes(text.as_bytes(), *seed);
            Value::Int64(i64::from_ne_bytes(
                ((u64::from(high) << 32) | u64::from(low)).to_ne_bytes(),
            ))
        }),
        ("gen_random_uuid" | "uuidv4", []) => Ok(Value::String(uuid::Uuid::new_v4().to_string())),
        ("uuidv7", []) => Ok(Value::String(uuid::Uuid::now_v7().to_string())),
        ("uuid_extract_version", [value]) => text(value).map(|text| {
            uuid::Uuid::parse_str(text)
                .ok()
                .filter(|uuid| uuid.get_variant() == uuid::Variant::RFC4122)
                .map_or(Value::Null, |uuid| {
                    Value::Int64(i64::try_from(uuid.get_version_num()).unwrap_or_default())
                })
        }),
        ("uuid_extract_timestamp", [value]) => text(value).map(|text| {
            uuid::Uuid::parse_str(text)
                .ok()
                .filter(|uuid| uuid.get_variant() == uuid::Variant::RFC4122)
                .and_then(|uuid| uuid.get_timestamp())
                .and_then(|timestamp| {
                    let (seconds, nanos) = timestamp.to_unix();
                    i64::try_from(seconds)
                        .ok()
                        .and_then(|seconds| {
                            seconds
                                .checked_sub(
                                    crate::types::extended::temporal::calendar::UNIX_TO_POSTGRES_SECS,
                                )?
                                .checked_mul(1_000_000)
                        })
                        .and_then(|micros| micros.checked_add(i64::from(nanos / 1_000)))
                })
                .map_or(Value::Null, |instant| {
                    Value::Typed(TypedValue::TimestampTz(instant))
                })
        }),
        _ => Err(QueryError::General(format!(
            "function {name} with these arguments does not exist"
        ))),
    })
}

/// `pg_crc32` traditional (zlib) CRC-32.
fn crc32(bytes: &[u8]) -> u32 {
    reflected_crc(bytes, 0xEDB8_8320)
}

/// CRC-32C (Castagnoli).
fn crc32c(bytes: &[u8]) -> u32 {
    reflected_crc(bytes, 0x82F6_3B78)
}

fn reflected_crc(bytes: &[u8], polynomial: u32) -> u32 {
    !bytes.iter().fold(u32::MAX, |crc, byte| {
        (0..8).fold(crc ^ u32::from(*byte), |crc, _| {
            if crc & 1 == 1 {
                (crc >> 1) ^ polynomial
            } else {
                crc >> 1
            }
        })
    })
}

fn mix(a: &mut u32, b: &mut u32, c: &mut u32) {
    *a = a.wrapping_sub(*c);
    *a ^= c.rotate_left(4);
    *c = c.wrapping_add(*b);
    *b = b.wrapping_sub(*a);
    *b ^= a.rotate_left(6);
    *a = a.wrapping_add(*c);
    *c = c.wrapping_sub(*b);
    *c ^= b.rotate_left(8);
    *b = b.wrapping_add(*a);
    *a = a.wrapping_sub(*c);
    *a ^= c.rotate_left(16);
    *c = c.wrapping_add(*b);
    *b = b.wrapping_sub(*a);
    *b ^= a.rotate_left(19);
    *a = a.wrapping_add(*c);
    *c = c.wrapping_sub(*b);
    *c ^= b.rotate_left(4);
    *b = b.wrapping_add(*a);
}

fn final_mix(a: &mut u32, b: &mut u32, c: &mut u32) {
    *c ^= *b;
    *c = c.wrapping_sub(b.rotate_left(14));
    *a ^= *c;
    *a = a.wrapping_sub(c.rotate_left(11));
    *b ^= *a;
    *b = b.wrapping_sub(a.rotate_left(25));
    *c ^= *b;
    *c = c.wrapping_sub(b.rotate_left(16));
    *a ^= *c;
    *a = a.wrapping_sub(c.rotate_left(4));
    *b ^= *a;
    *b = b.wrapping_sub(a.rotate_left(14));
    *c ^= *b;
    *c = c.wrapping_sub(b.rotate_left(24));
}

fn word(bytes: &[u8]) -> u32 {
    bytes.iter().enumerate().fold(0, |word, (index, byte)| {
        word | (u32::from(*byte) << (8 * index))
    })
}

/// PostgreSQL's `hash_bytes_extended` (and `hash_bytes` with a zero seed):
/// returns the final `(b, c)` words.
pub(crate) fn hash_bytes(key: &[u8], seed: i64) -> (u32, u32) {
    let length = u32::try_from(key.len()).unwrap_or(u32::MAX);
    let initial = 0x9e37_79b9_u32.wrapping_add(length).wrapping_add(3_923_095);
    let (mut a, mut b, mut c) = (initial, initial, initial);
    if seed != 0 {
        let seed = u64::from_ne_bytes(seed.to_ne_bytes());
        a = a.wrapping_add(u32::try_from(seed >> 32).unwrap_or_default());
        b = b.wrapping_add(u32::try_from(seed & u64::from(u32::MAX)).unwrap_or_default());
        mix(&mut a, &mut b, &mut c);
    }
    let (chunks, tail) = key.as_chunks::<12>();
    for chunk in chunks {
        a = a.wrapping_add(word(&chunk[0..4]));
        b = b.wrapping_add(word(&chunk[4..8]));
        c = c.wrapping_add(word(&chunk[8..12]));
        mix(&mut a, &mut b, &mut c);
    }
    a = a.wrapping_add(word(&tail[..tail.len().min(4)]));
    if tail.len() > 4 {
        b = b.wrapping_add(word(&tail[4..tail.len().min(8)]));
    }
    if tail.len() > 8 {
        // The lowest byte of `c` is reserved for the length.
        c = c.wrapping_add(word(&tail[8..]) << 8);
    }
    final_mix(&mut a, &mut b, &mut c);
    (b, c)
}

#[cfg(test)]
mod tests {
    use super::{crc32, crc32c, evaluate};
    use crate::types::Value;

    #[test]
    fn should_return_postgresql_hashtext_vectors() {
        // Arrange
        let inputs = ["", "a", "abc", "abcdefghijkl", "abcdefghijklmnopq"];

        // Act
        let hashes: Vec<Value> = inputs
            .iter()
            .map(|input| {
                evaluate("hashtext", &[Value::String((*input).to_string())])
                    .expect("hash function")
                    .unwrap_or(Value::Null)
            })
            .collect();

        // Assert
        assert_eq!(
            hashes,
            [
                -1_477_818_771,
                1_075_015_857,
                -785_388_649,
                -1_586_087_212,
                1_648_114_242
            ]
            .map(Value::Int64)
        );
    }

    #[test]
    fn should_return_postgresql_hashtextextended_vectors() {
        // Arrange
        let cases = [
            ("abc", 0, -6_747_756_470_228_489_321_i64),
            ("abc", 1, 2_125_675_926_272_891_485),
            ("abcdefghijklmnopq", -5, -6_985_432_419_487_294_399),
        ];

        // Act
        let hashes: Vec<Value> = cases
            .iter()
            .map(|(input, seed, _)| {
                evaluate(
                    "hashtextextended",
                    &[Value::String((*input).to_string()), Value::Int64(*seed)],
                )
                .expect("hash function")
                .unwrap_or(Value::Null)
            })
            .collect();

        // Assert
        assert_eq!(hashes, cases.map(|case| Value::Int64(case.2)));
    }

    #[test]
    fn should_return_postgresql_crc_vectors() {
        // Arrange
        let input = b"abc";

        // Act
        let checksums = (crc32(input), crc32c(input));

        // Assert
        assert_eq!(checksums, (891_568_578, 910_901_175));
    }
}
