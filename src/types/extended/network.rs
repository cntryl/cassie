//! `inet`, `cidr`, `macaddr` and `macaddr8`: input, output, ordering and
//! binary framing, following PostgreSQL's `network.c` and `mac*.c`.
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use super::TypeInputError;

/// `PGSQL_AF_INET`, the IPv4 family code of the binary format.
pub const FAMILY_V4: u8 = 2;
/// `PGSQL_AF_INET6`, the IPv6 family code of the binary format.
pub const FAMILY_V6: u8 = 3;

/// An `inet` or `cidr` value: family, mask length and address bytes (IPv4
/// uses the first four).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetAddr {
    pub family: u8,
    pub bits: u8,
    pub addr: [u8; 16],
}

/// The PostgreSQL `network_cmp` order: family, network bits, mask length,
/// then the full address.
pub type NetworkKey = (u8, [u8; 16], u8, [u8; 16]);

impl NetAddr {
    #[must_use]
    pub const fn is_v4(self) -> bool {
        self.family == FAMILY_V4
    }

    /// The maximum mask length of the family.
    #[must_use]
    pub const fn width(self) -> u8 {
        if self.is_v4() {
            32
        } else {
            128
        }
    }

    #[must_use]
    pub const fn byte_len(self) -> usize {
        if self.is_v4() {
            4
        } else {
            16
        }
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.addr[..self.byte_len()]
    }

    /// The address with every bit after `bits` cleared.
    #[must_use]
    pub fn masked(self, bits: u8) -> [u8; 16] {
        let mut out = [0; 16];
        for (index, byte) in self.addr.iter().take(self.byte_len()).enumerate() {
            out[index] = byte & mask_byte(bits, index);
        }
        out
    }

    /// Whether no bit to the right of the mask is set (`addressOK`).
    #[must_use]
    pub fn host_bits_clear(self) -> bool {
        self.masked(self.bits) == self.addr
    }

    #[must_use]
    pub fn order_key(self) -> NetworkKey {
        (self.family, self.masked(self.bits), self.bits, self.addr)
    }

    /// The address as an unsigned integer of its family width.
    #[must_use]
    pub fn as_u128(self) -> u128 {
        self.bytes()
            .iter()
            .fold(0_u128, |value, byte| (value << 8) | u128::from(*byte))
    }

    #[must_use]
    pub fn with_u128(self, value: u128) -> Self {
        let mut addr = [0; 16];
        let len = self.byte_len();
        let bytes = value.to_be_bytes();
        addr[..len].copy_from_slice(&bytes[16 - len..]);
        Self { addr, ..self }
    }
}

/// The mask byte at `index` for a prefix of `bits` bits.
#[must_use]
pub fn mask_byte(bits: u8, index: usize) -> u8 {
    let start = index * 8;
    let bits = usize::from(bits);
    if bits >= start + 8 {
        0xff
    } else if bits <= start {
        0
    } else {
        0xff_u8 << (8 - (bits - start))
    }
}

fn syntax(type_name: &str, text: &str) -> TypeInputError {
    TypeInputError::new(
        "22P02",
        format!("invalid input syntax for type {type_name}: \"{text}\""),
    )
}

/// `inet_in` / `cidr_in`.
///
/// # Errors
/// Returns `22P02` for malformed text, an out-of-range mask, or (for
/// `cidr`) bits set to the right of the mask.
pub fn parse_network(text: &str, is_cidr: bool) -> Result<NetAddr, TypeInputError> {
    let type_name = if is_cidr { "cidr" } else { "inet" };
    let parsed = if text.contains(':') {
        parse_v6(text)
    } else if is_cidr {
        parse_v4_cidr(text)
    } else {
        parse_v4_inet(text)
    }
    .ok_or_else(|| syntax(type_name, text))?;
    if is_cidr && !parsed.host_bits_clear() {
        return Err(TypeInputError::new(
            "22P02",
            format!("invalid cidr value: \"{text}\""),
        ));
    }
    Ok(parsed)
}

fn digits_value(digits: &str, limit: u32) -> Option<u32> {
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut value = 0_u32;
    for byte in digits.bytes() {
        value = value * 10 + u32::from(byte - b'0');
        if value > limit {
            return None;
        }
    }
    Some(value)
}

fn split_prefix(text: &str, limit: u32) -> Option<(&str, Option<u8>)> {
    match text.split_once('/') {
        Some((address, bits)) => {
            let bits = digits_value(bits, limit)?;
            Some((address, Some(u8::try_from(bits).ok()?)))
        }
        None => Some((text, None)),
    }
}

fn v4_octets(address: &str) -> Option<Vec<u8>> {
    let mut octets = Vec::new();
    for part in address.split('.') {
        let value = digits_value(part, 255)?;
        octets.push(u8::try_from(value).ok()?);
    }
    (1..=4).contains(&octets.len()).then_some(octets)
}

fn v4(octets: &[u8], bits: u8) -> NetAddr {
    let mut addr = [0; 16];
    addr[..octets.len()].copy_from_slice(octets);
    NetAddr {
        family: FAMILY_V4,
        bits,
        addr,
    }
}

fn parse_v4_inet(text: &str) -> Option<NetAddr> {
    let (address, bits) = split_prefix(text, 32)?;
    let octets = v4_octets(address)?;
    let bits = match bits {
        Some(bits) => bits,
        None if octets.len() == 4 => 32,
        None => return None,
    };
    (usize::from(bits) / 8 <= octets.len()).then(|| v4(&octets, bits))
}

fn parse_v4_cidr(text: &str) -> Option<NetAddr> {
    let (address, bits) = split_prefix(text, 32)?;
    let octets = v4_octets(address)?;
    let specified = u8::try_from(octets.len() * 8).ok()?;
    let bits = bits.unwrap_or_else(|| {
        let first = octets[0];
        let classful = match first {
            240.. => 32,
            224.. | 0..=127 => 8,
            192.. => 24,
            128.. => 16,
        };
        let widened = classful.max(specified);
        if widened == 8 && first == 224 {
            4
        } else {
            widened
        }
    });
    Some(v4(&octets, bits))
}

fn parse_v6(text: &str) -> Option<NetAddr> {
    let (address, bits) = split_prefix(text, 128)?;
    let parsed = address.parse::<std::net::Ipv6Addr>().ok()?;
    Some(NetAddr {
        family: FAMILY_V6,
        bits: bits.unwrap_or(128),
        addr: parsed.octets(),
    })
}

/// The address without its mask (`host`).
#[must_use]
pub fn format_address(value: NetAddr) -> String {
    if value.is_v4() {
        let bytes = value.bytes();
        format!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3])
    } else {
        format_v6(&value.addr)
    }
}

/// `inet_out` (mask omitted at full width) and `cidr_out` (always masked).
#[must_use]
pub fn format_network(value: NetAddr, is_cidr: bool) -> String {
    let address = format_address(value);
    if is_cidr || value.bits != value.width() {
        format!("{address}/{}", value.bits)
    } else {
        address
    }
}

fn format_v6(addr: &[u8; 16]) -> String {
    let words: Vec<u16> = addr
        .chunks(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();
    let (mut best, mut current) = ((None::<usize>, 0_usize), (None::<usize>, 0_usize));
    for (index, word) in words.iter().enumerate() {
        if *word == 0 {
            current = (current.0.or(Some(index)), current.1 + 1);
        } else if current.0.is_some() {
            if best.0.is_none() || current.1 > best.1 {
                best = current;
            }
            current = (None, 0);
        }
    }
    if current.0.is_some() && (best.0.is_none() || current.1 > best.1) {
        best = current;
    }
    if best.1 < 2 {
        best = (None, 0);
    }
    let mut out = String::new();
    for (index, word) in words.iter().enumerate() {
        if let Some(base) = best.0 {
            if (base..base + best.1).contains(&index) {
                if index == base {
                    out.push(':');
                }
                continue;
            }
        }
        if index != 0 {
            out.push(':');
        }
        if index == 6
            && best.0 == Some(0)
            && (best.1 == 6
                || (best.1 == 7 && words[7] != 1)
                || (best.1 == 5 && words[5] == 0xffff))
        {
            let _ = write!(out, "{}.{}.{}.{}", addr[12], addr[13], addr[14], addr[15]);
            break;
        }
        let _ = write!(out, "{word:x}");
    }
    if best.0.is_some_and(|base| base + best.1 == 8) {
        out.push(':');
    }
    out
}

/// `abbrev(cidr)`: `inet_cidr_ntop`, which drops trailing zero octets.
#[must_use]
pub fn abbreviate_cidr(value: NetAddr) -> String {
    if value.is_v4() {
        abbreviate_cidr_v4(value)
    } else {
        abbreviate_cidr_v6(value)
    }
}

fn abbreviate_cidr_v4(value: NetAddr) -> String {
    let bits = usize::from(value.bits);
    if bits == 0 {
        return "0/0".to_string();
    }
    let mut parts: Vec<String> = value.addr[..bits / 8].iter().map(u8::to_string).collect();
    let partial = bits % 8;
    if partial > 0 {
        parts.push((value.addr[bits / 8] & mask_byte(value.bits, bits / 8)).to_string());
    }
    format!("{}/{}", parts.join("."), value.bits)
}

fn abbreviate_cidr_v6(value: NetAddr) -> String {
    let bits = usize::from(value.bits);
    if bits == 0 {
        return "::/0".to_string();
    }
    let s = value.masked(value.bits);
    let words = bits.div_ceil(16).max(2);
    let (mut zero_s, mut zero_l, mut tmp_s, mut tmp_l) = (0, 0, 0, 0);
    for word in 0..words {
        if s[word * 2] | s[word * 2 + 1] == 0 {
            if tmp_l == 0 {
                tmp_s = word;
            }
            tmp_l += 1;
        } else if tmp_l != 0 && zero_l < tmp_l {
            zero_s = tmp_s;
            zero_l = tmp_l;
            tmp_l = 0;
        }
    }
    if tmp_l != 0 && zero_l < tmp_l {
        zero_s = tmp_s;
        zero_l = tmp_l;
    }
    let is_ipv4 = zero_l != words
        && zero_s == 0
        && (zero_l == 6
            || (zero_l == 5 && s[10] == 0xff && s[11] == 0xff)
            || (zero_l == 7 && s[14] != 0 && s[15] != 1));
    let mut out = String::new();
    for word in 0..words {
        if zero_l != 0 && word >= zero_s && word < zero_s + zero_l {
            if word == zero_s {
                out.push(':');
            }
            if word == words - 1 {
                out.push(':');
            }
            continue;
        }
        if is_ipv4 && word > 5 {
            out.push(if word == 6 { ':' } else { '.' });
            out.push_str(&s[word * 2].to_string());
            if word != 7 || bits > 120 {
                out.push('.');
                out.push_str(&s[word * 2 + 1].to_string());
            }
        } else {
            if !out.is_empty() {
                out.push(':');
            }
            let _ = write!(
                out,
                "{:x}",
                u16::from_be_bytes([s[word * 2], s[word * 2 + 1]])
            );
        }
    }
    format!("{out}/{}", value.bits)
}

/// `inet_send` / `cidr_send`: family, bits, is-cidr flag, length, address.
#[must_use]
pub fn send_network(value: NetAddr, is_cidr: bool) -> Vec<u8> {
    let mut out = vec![
        value.family,
        value.bits,
        u8::from(is_cidr),
        u8::try_from(value.byte_len()).unwrap_or(16),
    ];
    out.extend_from_slice(value.bytes());
    out
}

/// The message PostgreSQL reports when a binary frame ends early.
pub const INSUFFICIENT_DATA: &str = "insufficient data left in message";

/// `inet_recv` / `cidr_recv`.
///
/// # Errors
/// Returns PostgreSQL's receive error for a short frame or invalid fields.
pub fn receive_network(bytes: &[u8], is_cidr: bool) -> Result<NetAddr, String> {
    let type_name = if is_cidr { "cidr" } else { "inet" };
    let [family, bits, _, length, address @ ..] = bytes else {
        return Err(INSUFFICIENT_DATA.to_string());
    };
    let (width, size) = match *family {
        FAMILY_V4 => (32, 4),
        FAMILY_V6 => (128, 16),
        _ => {
            return Err(format!(
                "invalid address family in external \"{type_name}\" value"
            ))
        }
    };
    if *bits > width {
        return Err(format!("invalid bits in external \"{type_name}\" value"));
    }
    if usize::from(*length) != size {
        return Err(format!("invalid length in external \"{type_name}\" value"));
    }
    if address.len() < size {
        return Err(INSUFFICIENT_DATA.to_string());
    }
    if address.len() > size {
        return Err("incorrect binary data format".to_string());
    }
    let mut addr = [0; 16];
    addr[..size].copy_from_slice(address);
    let value = NetAddr {
        family: *family,
        bits: *bits,
        addr,
    };
    if is_cidr && !value.host_bits_clear() {
        return Err("invalid external \"cidr\" value".to_string());
    }
    Ok(value)
}

/// `macaddr_in` / `macaddr8_in`: hexadecimal byte pairs, optionally
/// separated by one consistent `:`, `-` or `.`.
///
/// # Errors
/// Returns `22P02` for malformed text.
pub fn parse_mac_bytes(text: &str, type_name: &str) -> Result<Vec<u8>, TypeInputError> {
    let error = || syntax(type_name, text);
    let trimmed = text.trim_matches(|c: char| c.is_ascii_whitespace());
    let separator = trimmed.chars().find(|c| matches!(c, ':' | '-' | '.'));
    let mut bytes = Vec::new();
    let mut digits = Vec::new();
    let mut previous_separator = true;
    for character in trimmed.chars() {
        if Some(character) == separator {
            if previous_separator || digits.len() % 2 != 0 {
                return Err(error());
            }
            previous_separator = true;
            continue;
        }
        let digit = character.to_digit(16).ok_or_else(error)?;
        digits.push(u8::try_from(digit).map_err(|_| error())?);
        previous_separator = false;
        if digits.len() == 2 {
            bytes.push(digits[0] << 4 | digits[1]);
            digits.clear();
        }
    }
    if previous_separator || !digits.is_empty() {
        return Err(error());
    }
    Ok(bytes)
}

/// `macaddr_in`.
///
/// # Errors
/// Returns `22P02` for malformed text.
pub fn parse_macaddr(text: &str) -> Result<[u8; 6], TypeInputError> {
    let bytes = parse_mac_bytes(text, "macaddr")?;
    <[u8; 6]>::try_from(bytes.as_slice()).map_err(|_| syntax("macaddr", text))
}

/// `macaddr8_in`: six-byte input becomes the EUI-64 form with `ff:fe`.
///
/// # Errors
/// Returns `22P02` for malformed text.
pub fn parse_macaddr8(text: &str) -> Result<[u8; 8], TypeInputError> {
    let bytes = parse_mac_bytes(text, "macaddr8")?;
    match bytes.len() {
        8 => <[u8; 8]>::try_from(bytes.as_slice()).map_err(|_| syntax("macaddr8", text)),
        6 => Ok(macaddr_to_macaddr8(
            <[u8; 6]>::try_from(bytes.as_slice()).map_err(|_| syntax("macaddr8", text))?,
        )),
        _ => Err(syntax("macaddr8", text)),
    }
}

/// `macaddrtomacaddr8`: inserts `ff:fe` in the middle.
#[must_use]
pub const fn macaddr_to_macaddr8(mac: [u8; 6]) -> [u8; 8] {
    [mac[0], mac[1], mac[2], 0xff, 0xfe, mac[3], mac[4], mac[5]]
}

/// `macaddr8tomacaddr`.
///
/// # Errors
/// Returns `22003` unless bytes four and five are `ff:fe`.
pub fn macaddr8_to_macaddr(mac: [u8; 8]) -> Result<[u8; 6], TypeInputError> {
    if mac[3] != 0xff || mac[4] != 0xfe {
        return Err(TypeInputError::new(
            "22003",
            "macaddr8 data out of range to convert to macaddr".to_string(),
        ));
    }
    Ok([mac[0], mac[1], mac[2], mac[5], mac[6], mac[7]])
}

#[must_use]
pub fn format_mac(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// `macaddr_recv` / `macaddr8_recv`.
///
/// # Errors
/// Returns PostgreSQL's receive error for a short or long frame.
pub fn receive_mac<const N: usize>(bytes: &[u8]) -> Result<[u8; N], String> {
    if bytes.len() < N {
        return Err(INSUFFICIENT_DATA.to_string());
    }
    <[u8; N]>::try_from(bytes).map_err(|_| "incorrect binary data format".to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        abbreviate_cidr, format_network, parse_macaddr, parse_macaddr8, parse_network,
        receive_network, send_network,
    };

    fn round_trip(text: &str, is_cidr: bool) -> String {
        format_network(
            parse_network(text, is_cidr).expect("valid network"),
            is_cidr,
        )
    }

    #[test]
    fn should_format_network_input_like_postgresql() {
        // Arrange
        let cases = [
            ("192.168.1.5/24", false, "192.168.1.5/24"),
            ("192.0.2.9", false, "192.0.2.9"),
            ("10/8", false, "10.0.0.0/8"),
            ("01.02.03.04", false, "1.2.3.4"),
            (
                "2001:0db8:0000:0000:0000:ff00:0042:8329",
                false,
                "2001:db8::ff00:42:8329",
            ),
            ("0:0:1:0:0:1:0:0", false, "::1:0:0:1:0:0"),
            ("::ffff:1.2.3.4", false, "::ffff:1.2.3.4"),
            ("::", false, "::"),
            ("1::", false, "1::"),
            ("10", true, "10.0.0.0/8"),
            ("224", true, "224.0.0.0/4"),
            ("224.1", true, "224.1.0.0/16"),
            ("240", true, "240.0.0.0/32"),
            ("128.1", true, "128.1.0.0/16"),
            ("192.168.1/24", true, "192.168.1.0/24"),
            ("ffff::", true, "ffff::/128"),
        ];

        // Act
        let formatted: Vec<String> = cases
            .iter()
            .map(|(text, is_cidr, _)| round_trip(text, *is_cidr))
            .collect();

        // Assert
        let expected: Vec<&str> = cases.iter().map(|case| case.2).collect();
        assert_eq!(formatted, expected);
    }

    #[test]
    fn should_reject_invalid_network_input_with_sqlstates() {
        // Arrange
        let cases = [
            ("10", false),
            ("1.2.3", false),
            ("1.2.3.4/", false),
            ("192.0.2.256", false),
            ("192.0.2.1/33", false),
            ("2001:db8::1/129", false),
            ("192.168.1.5/24", true),
        ];

        // Act
        let states: Vec<_> = cases
            .iter()
            .map(|(text, is_cidr)| parse_network(text, *is_cidr).map_err(|error| error.sqlstate))
            .collect();

        // Assert
        assert!(states.iter().all(|state| *state == Err("22P02")));
    }

    #[test]
    fn should_abbreviate_cidr_values_like_postgresql() {
        // Arrange
        let cases = [
            ("10.1.0.0/16", "10.1/16"),
            ("10.0.0.0/8", "10/8"),
            ("1.2.0.0/23", "1.2.0/23"),
            ("0.0.0.0/0", "0/0"),
            ("2001:db8::/32", "2001:db8/32"),
            ("2001:db8:1:0:1::/80", "2001:db8:1::1/80"),
            ("2001:db8:8000::/33", "2001:db8:8000/33"),
            ("::1/128", "::1/128"),
            ("::/0", "::/0"),
        ];

        // Act
        let abbreviated: Vec<String> = cases
            .iter()
            .map(|(text, _)| abbreviate_cidr(parse_network(text, true).expect("valid cidr")))
            .collect();

        // Assert
        let expected: Vec<&str> = cases.iter().map(|case| case.1).collect();
        assert_eq!(abbreviated, expected);
    }

    #[test]
    fn should_round_trip_network_binary_frames() {
        // Arrange
        let value = parse_network("2001:db8::1/64", false).expect("valid inet");

        // Act
        let frame = send_network(value, false);

        // Assert
        assert_eq!(&frame[..4], &[3, 64, 0, 16]);
        assert_eq!(receive_network(&frame, false), Ok(value));
        assert!(receive_network(&[2, 24, 1, 4, 192, 0, 2, 9], true).is_err());
    }

    #[test]
    fn should_accept_mac_address_spellings() {
        // Arrange
        let spellings = [
            "08:00:2b:01:02:03",
            "0800.2b01.0203",
            "08002b010203",
            "08-00-2b-01-02-03",
        ];

        // Act
        let parsed: Vec<_> = spellings.iter().map(|text| parse_macaddr(text)).collect();

        // Assert
        assert!(parsed
            .iter()
            .all(|value| *value == Ok([0x08, 0x00, 0x2b, 0x01, 0x02, 0x03])));
        assert_eq!(
            parse_macaddr8("08:00:2b:01:02:03"),
            Ok([0x08, 0x00, 0x2b, 0xff, 0xfe, 0x01, 0x02, 0x03])
        );
        assert!(parse_macaddr("08:00:2b:01:02:zz").is_err());
    }
}
