//! Text input, casts and binary receive for the network, MAC address,
//! `pg_lsn` and bit-string families.
use super::{bit_string, network, pg_lsn, ExtendedType, TypeInputError};
use crate::types::typed::TypedValue;
use crate::types::Value;

/// `typinput` with the type's length modifier applied as an assignment.
pub(super) fn parse_text(kind: ExtendedType, text: &str) -> Result<TypedValue, TypeInputError> {
    Ok(match kind {
        ExtendedType::Inet => TypedValue::Inet(network::parse_network(text, false)?),
        ExtendedType::Cidr => TypedValue::Cidr(network::parse_network(text, true)?),
        ExtendedType::Macaddr => TypedValue::Macaddr(network::parse_macaddr(text)?),
        ExtendedType::Macaddr8 => TypedValue::Macaddr8(network::parse_macaddr8(text)?),
        ExtendedType::PgLsn => TypedValue::PgLsn(pg_lsn::parse_text(text)?),
        ExtendedType::Bit { length } => TypedValue::Bit(bit_string::check_length(
            bit_string::parse_text(text)?,
            length,
            false,
        )?),
        ExtendedType::VarBit { length } => TypedValue::VarBit(bit_string::check_length(
            bit_string::parse_text(text)?,
            length,
            true,
        )?),
        ExtendedType::Real
        | ExtendedType::Numeric(_)
        | ExtendedType::Date
        | ExtendedType::Time { .. }
        | ExtendedType::TimeTz { .. }
        | ExtendedType::Timestamp { .. }
        | ExtendedType::TimestampTz { .. }
        | ExtendedType::Interval { .. } => return kind.parse_text(text),
    })
}

fn unsupported(kind: ExtendedType) -> TypeInputError {
    TypeInputError::new("42846", format!("cannot cast value to {}", kind.name()))
}

/// `CAST(value AS kind)`: explicit conversions, including the length
/// coercion of bit strings and network/MAC address conversions.
pub(super) fn cast_from(kind: ExtendedType, value: &Value) -> Result<TypedValue, TypeInputError> {
    match (kind, value) {
        (ExtendedType::Bit { length } | ExtendedType::VarBit { length }, _) => {
            let varying = matches!(kind, ExtendedType::VarBit { .. });
            let bits = match value {
                Value::Typed(TypedValue::Bit(bits) | TypedValue::VarBit(bits)) => bits.clone(),
                Value::String(text) => bit_string::parse_text(text)?,
                Value::Int64(integer) if !varying => bit_string::BitString::from_integer(
                    *integer,
                    usize::try_from(length.unwrap_or(1)).unwrap_or(1),
                ),
                _ => return Err(unsupported(kind)),
            };
            let bits = bit_string::coerce_length(&bits, length, varying);
            Ok(if varying {
                TypedValue::VarBit(bits)
            } else {
                TypedValue::Bit(bits)
            })
        }
        (ExtendedType::Cidr, Value::Typed(TypedValue::Inet(address))) => {
            Ok(TypedValue::Cidr(network::NetAddr {
                addr: address.masked(address.bits),
                ..*address
            }))
        }
        (ExtendedType::Inet, Value::Typed(TypedValue::Cidr(address))) => {
            Ok(TypedValue::Inet(*address))
        }
        (ExtendedType::Macaddr8, Value::Typed(TypedValue::Macaddr(mac))) => {
            Ok(TypedValue::Macaddr8(network::macaddr_to_macaddr8(*mac)))
        }
        (ExtendedType::Macaddr, Value::Typed(TypedValue::Macaddr8(mac))) => {
            network::macaddr8_to_macaddr(*mac).map(TypedValue::Macaddr)
        }
        (_, Value::String(text)) => parse_text(kind, text),
        _ => Err(unsupported(kind)),
    }
}

/// `typreceive`.
pub(super) fn receive_binary(kind: ExtendedType, bytes: &[u8]) -> Result<TypedValue, String> {
    Ok(match kind {
        ExtendedType::Inet => TypedValue::Inet(network::receive_network(bytes, false)?),
        ExtendedType::Cidr => TypedValue::Cidr(network::receive_network(bytes, true)?),
        ExtendedType::Macaddr => TypedValue::Macaddr(network::receive_mac::<6>(bytes)?),
        ExtendedType::Macaddr8 => TypedValue::Macaddr8(network::receive_mac::<8>(bytes)?),
        ExtendedType::PgLsn => TypedValue::PgLsn(pg_lsn::receive_binary(bytes)?),
        ExtendedType::Bit { length } => {
            let bits = bit_string::receive_binary(bytes)?;
            TypedValue::Bit(
                bit_string::check_length(bits, length, false).map_err(|error| error.message)?,
            )
        }
        ExtendedType::VarBit { length } => {
            let bits = bit_string::receive_binary(bytes)?;
            TypedValue::VarBit(
                bit_string::check_length(bits, length, true).map_err(|error| error.message)?,
            )
        }
        ExtendedType::Real
        | ExtendedType::Numeric(_)
        | ExtendedType::Date
        | ExtendedType::Time { .. }
        | ExtendedType::TimeTz { .. }
        | ExtendedType::Timestamp { .. }
        | ExtendedType::TimestampTz { .. }
        | ExtendedType::Interval { .. } => return kind.receive_binary(bytes),
    })
}
