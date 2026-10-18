//! Operators and functions over `inet`, `cidr`, `macaddr` and `macaddr8`.
use super::network::{self, NetAddr};
use super::{ExtendedType, TypeInputError, TypedOperator};
use crate::types::typed::TypedValue;
use crate::types::{DataType, Value};

fn address(value: &Value) -> Option<(NetAddr, bool)> {
    match value {
        Value::Typed(TypedValue::Inet(address)) => Some((*address, false)),
        Value::Typed(TypedValue::Cidr(address)) => Some((*address, true)),
        _ => None,
    }
}

const fn is_network(kind: &DataType) -> bool {
    matches!(
        kind,
        DataType::Extended(ExtendedType::Inet | ExtendedType::Cidr)
    )
}

const fn is_mac(kind: &DataType) -> Option<ExtendedType> {
    match kind {
        DataType::Extended(kind @ (ExtendedType::Macaddr | ExtendedType::Macaddr8)) => Some(*kind),
        _ => None,
    }
}

const fn inet() -> DataType {
    DataType::Extended(ExtendedType::Inet)
}

const fn cidr() -> DataType {
    DataType::Extended(ExtendedType::Cidr)
}

fn integer(value: &Value) -> Option<i64> {
    match value {
        Value::Int64(value) => Some(*value),
        _ => None,
    }
}

const fn is_integer(kind: &DataType) -> bool {
    matches!(kind, DataType::SmallInt | DataType::Int | DataType::BigInt)
}

fn out_of_range() -> TypeInputError {
    TypeInputError::new("22003", "result is out of range".to_string())
}

fn different_sizes(verb: &str) -> TypeInputError {
    TypeInputError::new(
        "22023",
        format!("cannot {verb} inet values of different sizes"),
    )
}

/// The result type of a network operator, or `None` when it is not one.
#[must_use]
pub fn operator_type(op: TypedOperator, left: &DataType, right: &DataType) -> Option<DataType> {
    match op {
        TypedOperator::ShiftLeft
        | TypedOperator::ShiftRight
        | TypedOperator::ContainedOrEqual
        | TypedOperator::ContainsOrEqual
        | TypedOperator::Overlap
            if is_network(left) && is_network(right) =>
        {
            Some(DataType::Boolean)
        }
        TypedOperator::And | TypedOperator::Or if is_network(left) && is_network(right) => {
            Some(inet())
        }
        TypedOperator::And | TypedOperator::Or if is_mac(left).is_some() && left == right => {
            Some(left.clone())
        }
        TypedOperator::Add | TypedOperator::Sub if is_network(left) && is_integer(right) => {
            Some(inet())
        }
        TypedOperator::Add if is_integer(left) && is_network(right) => Some(inet()),
        TypedOperator::Sub if is_network(left) && is_network(right) => Some(DataType::BigInt),
        _ => None,
    }
}

/// Evaluates a network operator over two non-NULL operands.
#[must_use]
pub fn operator(
    op: TypedOperator,
    left: &Value,
    right: &Value,
) -> Option<Result<Value, TypeInputError>> {
    if let (Some((left, _)), Some((right, _))) = (address(left), address(right)) {
        return network_pair(op, left, right);
    }
    if let (Value::Typed(left), Value::Typed(right)) = (left, right) {
        return mac_pair(op, left, right).map(Ok);
    }
    match (op, address(left), address(right)) {
        (TypedOperator::Add, Some((address, _)), None) => {
            integer(right).map(|offset| add(address, i128::from(offset)))
        }
        (TypedOperator::Add, None, Some((address, _))) => {
            integer(left).map(|offset| add(address, i128::from(offset)))
        }
        (TypedOperator::Sub, Some((address, _)), None) => {
            integer(right).map(|offset| add(address, -i128::from(offset)))
        }
        _ => None,
    }
}

fn add(address: NetAddr, offset: i128) -> Result<Value, TypeInputError> {
    let current = address.as_u128();
    let max = if address.is_v4() {
        u128::from(u32::MAX)
    } else {
        u128::MAX
    };
    let result = if offset >= 0 {
        current.checked_add(offset.unsigned_abs())
    } else {
        current.checked_sub(offset.unsigned_abs())
    }
    .filter(|result| *result <= max)
    .ok_or_else(out_of_range)?;
    Ok(Value::Typed(TypedValue::Inet(address.with_u128(result))))
}

fn difference(left: NetAddr, right: NetAddr) -> Option<i64> {
    let (left, right) = (left.as_u128(), right.as_u128());
    if left >= right {
        i64::try_from(left - right).ok()
    } else {
        i64::try_from(right - left).ok().map(|value| -value)
    }
}

fn network_pair(
    op: TypedOperator,
    left: NetAddr,
    right: NetAddr,
) -> Option<Result<Value, TypeInputError>> {
    let same = left.family == right.family;
    let prefix_equal = |bits: u8| left.masked(bits) == right.masked(bits);
    let result = match op {
        TypedOperator::ShiftLeft => {
            Value::Bool(same && left.bits > right.bits && prefix_equal(right.bits))
        }
        TypedOperator::ContainedOrEqual => {
            Value::Bool(same && left.bits >= right.bits && prefix_equal(right.bits))
        }
        TypedOperator::ShiftRight => {
            Value::Bool(same && left.bits < right.bits && prefix_equal(left.bits))
        }
        TypedOperator::ContainsOrEqual => {
            Value::Bool(same && left.bits <= right.bits && prefix_equal(left.bits))
        }
        TypedOperator::Overlap => Value::Bool(same && prefix_equal(left.bits.min(right.bits))),
        TypedOperator::And | TypedOperator::Or => {
            if !same {
                let verb = if op == TypedOperator::And {
                    "AND"
                } else {
                    "OR"
                };
                return Some(Err(different_sizes(verb)));
            }
            let mut addr = [0; 16];
            for (index, byte) in addr.iter_mut().enumerate() {
                *byte = if op == TypedOperator::And {
                    left.addr[index] & right.addr[index]
                } else {
                    left.addr[index] | right.addr[index]
                };
            }
            Value::Typed(TypedValue::Inet(NetAddr {
                family: left.family,
                bits: left.bits.max(right.bits),
                addr,
            }))
        }
        TypedOperator::Sub => {
            if !same {
                return Some(Err(different_sizes("subtract")));
            }
            return Some(
                difference(left, right)
                    .map(Value::Int64)
                    .ok_or_else(out_of_range),
            );
        }
        _ => return None,
    };
    Some(Ok(result))
}

fn mac_pair(op: TypedOperator, left: &TypedValue, right: &TypedValue) -> Option<Value> {
    let combine = |left: &[u8], right: &[u8]| -> Vec<u8> {
        left.iter()
            .zip(right)
            .map(|(left, right)| {
                if op == TypedOperator::And {
                    left & right
                } else {
                    left | right
                }
            })
            .collect()
    };
    if !matches!(op, TypedOperator::And | TypedOperator::Or) {
        return None;
    }
    match (left, right) {
        (TypedValue::Macaddr(left), TypedValue::Macaddr(right)) => {
            <[u8; 6]>::try_from(combine(left, right).as_slice())
                .ok()
                .map(|mac| Value::Typed(TypedValue::Macaddr(mac)))
        }
        (TypedValue::Macaddr8(left), TypedValue::Macaddr8(right)) => {
            <[u8; 8]>::try_from(combine(left, right).as_slice())
                .ok()
                .map(|mac| Value::Typed(TypedValue::Macaddr8(mac)))
        }
        _ => None,
    }
}

/// `~` over a network or MAC address value.
#[must_use]
pub fn complement(value: &TypedValue) -> Option<TypedValue> {
    match value {
        TypedValue::Inet(address) | TypedValue::Cidr(address) => {
            let mut addr = [0; 16];
            for (index, byte) in address.bytes().iter().enumerate() {
                addr[index] = !byte;
            }
            Some(TypedValue::Inet(NetAddr { addr, ..*address }))
        }
        TypedValue::Macaddr(mac) => Some(TypedValue::Macaddr(mac.map(|byte| !byte))),
        TypedValue::Macaddr8(mac) => Some(TypedValue::Macaddr8(mac.map(|byte| !byte))),
        _ => None,
    }
}

/// The result type of a network function for the argument types.
#[must_use]
pub fn function_type(name: &str, args: &[DataType]) -> Option<DataType> {
    let first = args.first()?;
    if let Some(kind) = is_mac(first) {
        return match (name, args.len(), kind) {
            ("trunc", 1, _) | ("macaddr8_set7bit", 1, ExtendedType::Macaddr8) => {
                Some(first.clone())
            }
            _ => None,
        };
    }
    if !is_network(first) {
        return None;
    }
    match (name, args.len()) {
        ("host" | "text" | "abbrev", 1) => Some(DataType::Text),
        ("masklen" | "family", 1) => Some(DataType::Int),
        ("netmask" | "hostmask" | "broadcast", 1) => Some(inet()),
        ("network", 1) | ("inet_merge", 2) => Some(cidr()),
        ("set_masklen", 2) => Some(first.clone()),
        ("inet_same_family", 2) => Some(DataType::Boolean),
        _ => None,
    }
}

/// Evaluates a network or MAC address function; `None` when `name` with
/// these arguments is not one.
#[must_use]
pub fn function(name: &str, args: &[Value]) -> Option<Result<Value, TypeInputError>> {
    match (name, args) {
        ("trunc", [Value::Typed(TypedValue::Macaddr(mac))]) => {
            Some(Ok(Value::Typed(TypedValue::Macaddr([
                mac[0], mac[1], mac[2], 0, 0, 0,
            ]))))
        }
        ("trunc", [Value::Typed(TypedValue::Macaddr8(mac))]) => {
            Some(Ok(Value::Typed(TypedValue::Macaddr8([
                mac[0], mac[1], mac[2], 0, 0, 0, 0, 0,
            ]))))
        }
        ("macaddr8_set7bit", [Value::Typed(TypedValue::Macaddr8(mac))]) => {
            let mut mac = *mac;
            mac[0] |= 0x02;
            Some(Ok(Value::Typed(TypedValue::Macaddr8(mac))))
        }
        (_, [value]) => address(value).and_then(|(address, is_cidr)| unary(name, address, is_cidr)),
        (_, [left, right]) => {
            let (address, is_cidr) = address(left)?;
            if let Some((other, _)) = self::address(right) {
                return binary(name, address, other);
            }
            (name == "set_masklen")
                .then(|| integer(right).map(|bits| set_masklen(address, is_cidr, bits)))
                .flatten()
        }
        _ => None,
    }
}

fn unary(name: &str, address: NetAddr, is_cidr: bool) -> Option<Result<Value, TypeInputError>> {
    let width = address.width();
    let full = |addr: [u8; 16]| {
        Value::Typed(TypedValue::Inet(NetAddr {
            bits: width,
            addr,
            ..address
        }))
    };
    let netmask = NetAddr {
        addr: [0xff; 16],
        ..address
    }
    .masked(address.bits);
    let mut hostmask = [0; 16];
    for (index, byte) in hostmask.iter_mut().take(address.byte_len()).enumerate() {
        *byte = !netmask[index];
    }
    let value = match name {
        "host" => Value::String(network::format_address(address)),
        "text" => Value::String(format!(
            "{}/{}",
            network::format_address(address),
            address.bits
        )),
        "abbrev" if is_cidr => Value::String(network::abbreviate_cidr(address)),
        "abbrev" => Value::String(network::format_network(address, false)),
        "masklen" => Value::Int64(i64::from(address.bits)),
        "family" => Value::Int64(if address.is_v4() { 4 } else { 6 }),
        "netmask" => full(netmask),
        "hostmask" => full(hostmask),
        "network" => Value::Typed(TypedValue::Cidr(NetAddr {
            addr: address.masked(address.bits),
            ..address
        })),
        "broadcast" => {
            let mut addr = address.addr;
            for (index, byte) in addr.iter_mut().take(address.byte_len()).enumerate() {
                *byte |= hostmask[index];
            }
            Value::Typed(TypedValue::Inet(NetAddr { addr, ..address }))
        }
        _ => return None,
    };
    Some(Ok(value))
}

fn binary(name: &str, left: NetAddr, right: NetAddr) -> Option<Result<Value, TypeInputError>> {
    match name {
        "inet_same_family" => Some(Ok(Value::Bool(left.family == right.family))),
        "inet_merge" => {
            if left.family != right.family {
                return Some(Err(TypeInputError::new(
                    "22023",
                    "cannot merge addresses from different families".to_string(),
                )));
            }
            let mut bits = left.bits.min(right.bits);
            while bits > 0 && left.masked(bits) != right.masked(bits) {
                bits -= 1;
            }
            Some(Ok(Value::Typed(TypedValue::Cidr(NetAddr {
                bits,
                addr: left.masked(bits),
                ..left
            }))))
        }
        _ => None,
    }
}

fn set_masklen(address: NetAddr, is_cidr: bool, bits: i64) -> Result<Value, TypeInputError> {
    let width = address.width();
    let bits = if bits == -1 { i64::from(width) } else { bits };
    let bits = u8::try_from(bits)
        .ok()
        .filter(|bits| *bits <= width)
        .ok_or_else(|| TypeInputError::new("22023", format!("invalid mask length: {bits}")))?;
    let value = NetAddr { bits, ..address };
    Ok(Value::Typed(if is_cidr {
        TypedValue::Cidr(NetAddr {
            addr: value.masked(bits),
            ..value
        })
    } else {
        TypedValue::Inet(value)
    }))
}

#[cfg(test)]
mod tests {
    use super::{function, operator};
    use crate::types::extended::{network::parse_network, TypedOperator};
    use crate::types::typed::TypedValue;
    use crate::types::Value;

    fn inet(text: &str) -> Value {
        Value::Typed(TypedValue::Inet(
            parse_network(text, false).expect("valid inet"),
        ))
    }

    fn cidr(text: &str) -> Value {
        Value::Typed(TypedValue::Cidr(
            parse_network(text, true).expect("valid cidr"),
        ))
    }

    fn text(value: &Value) -> String {
        match value {
            Value::Typed(value) => value.text(),
            Value::String(value) => value.clone(),
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn should_compute_ipv6_mask_functions_like_postgresql() {
        // Arrange
        let address = inet("2001:db8::1/64");
        let names = ["host", "network", "broadcast", "hostmask", "netmask"];

        // Act
        let results: Vec<String> = names
            .iter()
            .map(|name| {
                text(
                    &function(name, std::slice::from_ref(&address))
                        .expect("function")
                        .expect("ok"),
                )
            })
            .collect();

        // Assert
        assert_eq!(
            results,
            [
                "2001:db8::1",
                "2001:db8::/64",
                "2001:db8::ffff:ffff:ffff:ffff/64",
                "::ffff:ffff:ffff:ffff",
                "ffff:ffff:ffff:ffff::"
            ]
        );
    }

    #[test]
    fn should_merge_networks_to_the_smallest_common_cidr() {
        // Arrange
        let cases = [
            (
                cidr("2001:db8:1::/64"),
                cidr("2001:db8:1:1::/64"),
                "2001:db8:1::/63",
            ),
            (
                inet("2001:db8::1/128"),
                inet("2001:db8::3/128"),
                "2001:db8::/126",
            ),
            (
                inet("192.168.1.5/24"),
                inet("192.168.2.5/24"),
                "192.168.0.0/22",
            ),
        ];

        // Act
        let merged: Vec<String> = cases
            .iter()
            .map(|(left, right, _)| {
                text(
                    &function("inet_merge", &[left.clone(), right.clone()])
                        .expect("f")
                        .expect("ok"),
                )
            })
            .collect();

        // Assert
        assert_eq!(merged, cases.map(|case| case.2));
    }

    #[test]
    fn should_reject_address_arithmetic_overflow() {
        // Arrange
        let maximum = inet("255.255.255.255");
        let minimum = inet("0.0.0.0");

        // Act
        let up = operator(TypedOperator::Add, &maximum, &Value::Int64(1)).expect("operator");
        let down = operator(TypedOperator::Sub, &minimum, &Value::Int64(1)).expect("operator");
        let difference = operator(TypedOperator::Sub, &maximum, &minimum).expect("operator");

        // Assert
        assert_eq!(up.map_err(|error| error.sqlstate), Err("22003"));
        assert_eq!(down.map_err(|error| error.sqlstate), Err("22003"));
        assert_eq!(difference, Ok(Value::Int64(4_294_967_295)));
    }
}
